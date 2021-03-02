#!/usr/bin/python
# Copyright (C) 2019-2020 Jelmer Vernooij <jelmer@jelmer.uk>
# encoding: utf-8
#
# This program is free software; you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation; either version 2 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program; if not, write to the Free Software
# Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA

import logging
import re
from typing import List, Iterator, Optional, Set

import os
from buildlog_consultant.apt import (
    find_apt_get_failure,
)

from .. import DetailedFailure, UnidentifiedError
from ..session import Session, run_with_tee


def run_apt(session: Session, args: List[str]) -> None:
    """Run apt."""
    args = ["apt", "-y"] + args
    retcode, lines = run_with_tee(session, args, cwd="/", user="root")
    if retcode == 0:
        return
    match, error = find_apt_get_failure(lines)
    if error is not None:
        raise DetailedFailure(retcode, args, error)
    while lines and lines[-1] == "":
        lines.pop(-1)
    raise UnidentifiedError(retcode, args, lines, secondary=match)


class FileSearcher(object):
    def search_files(self, path: str, regex: bool = False) -> Iterator[str]:
        raise NotImplementedError(self.search_files)


class AptManager(object):

    session: Session
    _searchers: Optional[List[FileSearcher]]

    def __init__(self, session):
        self.session = session
        self._apt_cache = None
        self._searchers = None

    def searchers(self):
        if self._searchers is None:
            self._searchers = [
                AptContentsFileSearcher.from_session(self.session),
                GENERATED_FILE_SEARCHER,
            ]
        return self._searchers

    def package_exists(self, package):
        if self._apt_cache is None:
            import apt

            self._apt_cache = apt.Cache(rootdir=self.session.location)
        return package in self._apt_cache

    def get_package_for_paths(self, paths, regex=False):
        logging.debug("Searching for packages containing %r", paths)
        # TODO(jelmer): Make sure we use whatever is configured in self.session
        return get_package_for_paths(paths, self.searchers(), regex=regex)

    def missing(self, packages):
        root = getattr(self.session, "location", "/")
        status_path = os.path.join(root, "var/lib/dpkg/status")
        missing = set(packages)
        import apt_pkg

        with apt_pkg.TagFile(status_path) as tagf:
            while missing:
                tagf.step()
                if not tagf.section:
                    break
                if tagf.section["Package"] in missing:
                    if tagf.section["Status"] == "install ok installed":
                        missing.remove(tagf.section["Package"])
        return list(missing)

    def install(self, packages: List[str]) -> None:
        logging.info("Installing using apt: %r", packages)
        packages = self.missing(packages)
        if packages:
            run_apt(self.session, ["install"] + packages)

    def satisfy(self, deps: List[str]) -> None:
        run_apt(self.session, ["satisfy"] + deps)


class ContentsFileNotFound(Exception):
    """The contents file was not found."""


class AptContentsFileSearcher(FileSearcher):
    def __init__(self):
        self._db = {}

    @classmethod
    def from_session(cls, session):
        logging.info("Loading apt contents information")
        # TODO(jelmer): what about sources.list.d?
        from aptsources.sourceslist import SourcesList

        sl = SourcesList()
        sl.load(os.path.join(session.location, "etc/apt/sources.list"))
        return cls.from_sources_list(
            sl,
            cache_dirs=[
                os.path.join(session.location, "var/lib/apt/lists"),
                "/var/lib/apt/lists",
            ],
        )

    def __setitem__(self, path, package):
        self._db[path] = package

    def search_files(self, path, regex=False):
        c = re.compile(path)
        for p, pkg in sorted(self._db.items()):
            if regex:
                if c.match(p):
                    yield pkg
            else:
                if path == p:
                    yield pkg

    def load_file(self, f):
        for line in f:
            (path, rest) = line.rsplit(maxsplit=1)
            package = rest.split(b"/")[-1]
            decoded_path = "/" + path.decode("utf-8", "surrogateescape")
            self[decoded_path] = package.decode("utf-8")

    @classmethod
    def _load_cache_file(cls, url, cache_dir):
        from urllib.parse import urlparse

        parsed = urlparse(url)
        p = os.path.join(
            cache_dir, parsed.hostname + parsed.path.replace("/", "_") + ".lz4"
        )
        if not os.path.exists(p):
            return None
        logging.debug("Loading cached contents file %s", p)
        import lz4.frame

        return lz4.frame.open(p, mode="rb")

    @classmethod
    def from_urls(cls, urls, cache_dirs=None):
        self = cls()
        for url, mandatory in urls:
            for cache_dir in cache_dirs or []:
                f = cls._load_cache_file(url, cache_dir)
                if f is not None:
                    self.load_file(f)
                    break
            else:
                if not mandatory and self._db:
                    logging.debug(
                        "Not attempting to fetch optional contents " "file %s", url
                    )
                else:
                    logging.debug("Fetching contents file %s", url)
                    try:
                        self.load_url(url)
                    except ContentsFileNotFound:
                        if mandatory:
                            logging.warning("Unable to fetch contents file %s", url)
                        else:
                            logging.debug(
                                "Unable to fetch optional contents file %s", url
                            )
        return self

    @classmethod
    def from_sources_list(cls, sl, cache_dirs=None):
        # TODO(jelmer): Use aptsources.sourceslist.SourcesList
        from .build import get_build_architecture

        # TODO(jelmer): Verify signatures, etc.
        urls = []
        arches = [(get_build_architecture(), True), ("all", False)]
        for source in sl.list:
            if source.invalid or source.disabled:
                continue
            if source.type == "deb-src":
                continue
            if source.type != "deb":
                logging.warning("Invalid line in sources: %r", source)
                continue
            base_url = source.uri.rstrip("/")
            name = source.dist.rstrip("/")
            components = source.comps
            if components:
                dists_url = base_url + "/dists"
            else:
                dists_url = base_url
            if components:
                for component in components:
                    for arch, mandatory in arches:
                        urls.append(
                            (
                                "%s/%s/%s/Contents-%s"
                                % (dists_url, name, component, arch),
                                mandatory,
                            )
                        )
            else:
                for arch, mandatory in arches:
                    urls.append(
                        (
                            "%s/%s/Contents-%s" % (dists_url, name.rstrip("/"), arch),
                            mandatory,
                        )
                    )
        return cls.from_urls(urls, cache_dirs=cache_dirs)

    @staticmethod
    def _get(url):
        from urllib.request import urlopen, Request

        request = Request(url, headers={"User-Agent": "Debian Janitor"})
        return urlopen(request)

    def load_url(self, url, allow_cache=True):
        from urllib.error import HTTPError

        for ext in [".xz", ".gz", ""]:
            try:
                response = self._get(url + ext)
            except HTTPError as e:
                if e.status == 404:
                    continue
                raise
            break
        else:
            raise ContentsFileNotFound(url)
        if ext == ".gz":
            import gzip

            f = gzip.GzipFile(fileobj=response)
        elif ext == ".xz":
            import lzma
            from io import BytesIO

            f = BytesIO(lzma.decompress(response.read()))
        elif response.headers.get_content_type() == "text/plain":
            f = response
        else:
            raise Exception(
                "Unknown content type %r" % response.headers.get_content_type()
            )
        self.load_file(f)


class GeneratedFileSearcher(FileSearcher):
    def __init__(self, db):
        self._db = db

    def search_files(self, path: str, regex: bool = False) -> Iterator[str]:
        for p, pkg in sorted(self._db.items()):
            if regex:
                if re.match(path, p):
                    yield pkg
            else:
                if path == p:
                    yield pkg


# TODO(jelmer): read from a file
GENERATED_FILE_SEARCHER = GeneratedFileSearcher(
    {
        "/etc/locale.gen": "locales",
        # Alternative
        "/usr/bin/rst2html": "/usr/share/docutils/scripts/python3/rst2html",
    }
)


def get_package_for_paths(
    paths: List[str], searchers: List[FileSearcher], regex: bool = False
) -> Optional[str]:
    candidates: Set[str] = set()
    for path in paths:
        for searcher in searchers:
            candidates.update(searcher.search_files(path, regex=regex))
        if candidates:
            break
    if len(candidates) == 0:
        logging.warning("No packages found that contain %r", paths)
        return None
    if len(candidates) > 1:
        logging.warning(
            "More than 1 packages found that contain %r: %r", path, candidates
        )
        # Euhr. Pick the one with the shortest name?
        return sorted(candidates, key=len)[0]
    else:
        return candidates.pop()

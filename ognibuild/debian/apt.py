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
from debian.deb822 import Release

from .. import DetailedFailure
from ..session import Session, run_with_tee

from .build import get_build_architecture


class UnidentifiedError(Exception):

    def __init__(self, retcode, argv, lines, secondary=None):
        self.retcode = retcode
        self.argv = argv
        self.lines = lines
        self.secondary = secondary


def run_apt(session: Session, args: List[str]) -> None:
    """Run apt."""
    args = ["apt", "-y"] + args
    retcode, lines = run_with_tee(session, args, cwd="/", user="root")
    if retcode == 0:
        return
    match, error = find_apt_get_failure(lines)
    if error is not None:
        raise DetailedFailure(retcode, args, error)
    if match is not None:
        raise UnidentifiedError(retcode, args, lines, secondary=(match.lineno, match.line))
    while lines and lines[-1] == "":
        lines.pop(-1)
    raise UnidentifiedError(retcode, args, lines)


class AptManager(object):

    session: Session

    def __init__(self, session):
        self.session = session

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
        packages = self.missing(packages)
        if packages:
            run_apt(self.session, ["install"] + packages)

    def satisfy(self, deps: List[str]) -> None:
        run_apt(self.session, ["satisfy"] + deps)


class FileSearcher(object):
    def search_files(self, path, regex=False):
        raise NotImplementedError(self.search_files)


class ContentsFileNotFound(Exception):
    """The contents file was not found."""


class AptContentsFileSearcher(FileSearcher):

    _user_agent = 'ognibuild/0.1'

    def __init__(self):
        self._db = {}

    @classmethod
    def from_env(cls):
        sources = os.environ["REPOSITORIES"].split(":")
        return cls.from_repositories(sources)

    def __setitem__(self, path, package):
        self._db[path] = package

    def search_files(self, path, regex=False):
        for p, pkg in sorted(self._db.items()):
            if regex:
                if re.match(path, p):
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
    def from_urls(cls, urls):
        self = cls()
        for url in urls:
            self.load_url(url)
        return self

    @classmethod
    def from_repositories(cls, sources):
        # TODO(jelmer): Verify signatures, etc.
        urls = []
        arches = [get_build_architecture(), "all"]
        for source in sources:
            parts = source.split(" ")
            if parts[0] != "deb":
                logging.warning("Invalid line in sources: %r", source)
                continue
            base_url = parts[1]
            name = parts[2]
            components = parts[3:]
            response = cls._get("%s/%s/Release" % (base_url, name))
            r = Release(response)
            desired_files = set()
            for component in components:
                for arch in arches:
                    desired_files.add("%s/Contents-%s" % (component, arch))
            for entry in r["MD5Sum"]:
                if entry["name"] in desired_files:
                    urls.append("%s/%s/%s" % (base_url, name, entry["name"]))
        return cls.from_urls(urls)

    @classmethod
    def _get(cls, url):
        from urllib.request import urlopen, Request

        request = Request(url, headers={"User-Agent": cls._user_agent})
        return urlopen(request)

    def load_url(self, url):
        from urllib.error import HTTPError

        try:
            response = self._get(url)
        except HTTPError as e:
            if e.status == 404:
                raise ContentsFileNotFound(url)
            raise
        if url.endswith(".gz"):
            import gzip

            f = gzip.GzipFile(fileobj=response)
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

    def search_files(self, path, regex=False):
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


_apt_file_searcher = None


def search_apt_file(path: str, regex: bool = False) -> Iterator[FileSearcher]:
    global _apt_file_searcher
    if _apt_file_searcher is None:
        # TODO(jelmer): cache file
        _apt_file_searcher = AptContentsFileSearcher.from_env()
    if _apt_file_searcher:
        yield from _apt_file_searcher.search_files(path, regex=regex)
    yield from GENERATED_FILE_SEARCHER.search_files(path, regex=regex)

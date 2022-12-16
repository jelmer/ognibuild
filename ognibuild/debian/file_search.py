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

import apt_pkg
import asyncio
from contextlib import suppress
from datetime import datetime
from debian.deb822 import Release
import os
import re
import subprocess
from typing import List, AsyncIterator
import logging


from .. import USER_AGENT
from ..session import Session


class FileSearcher:
    def search_files(
            self, path: str, regex: bool = False,
            case_insensitive: bool = False) -> AsyncIterator[str]:
        raise NotImplementedError(self.search_files)


class AptFileAccessError(Exception):
    """Apt file access error."""


class ContentsFileNotFound(Exception):
    """The contents file was not found."""


def read_contents_file(f):
    for line in f:
        (path, rest) = line.rsplit(maxsplit=1)
        yield path, rest


def contents_urls_from_sources_entry(source, arches, load_url):
    if source.invalid or source.disabled:
        return
    if source.type == "deb-src":
        return
    if source.type != "deb":
        logging.warning("Invalid line in sources: %r", source)
        return
    base_url = source.uri.rstrip("/")
    name = source.dist.rstrip("/")
    components = source.comps
    if components:
        dists_url = base_url + "/dists"
    else:
        dists_url = base_url
    inrelease_url = "%s/%s/InRelease" % (dists_url, name)
    try:
        response = load_url(inrelease_url)
    except FileNotFoundError:
        release_url = "%s/%s/Release" % (dists_url, name)
        try:
            response = load_url(release_url)
        except FileNotFoundError as e:
            logging.warning(
                "Unable to download %s or %s: %s", inrelease_url,
                release_url, e
            )
            return

    existing_names = {}
    release = Release(response.read())
    for hn in ["MD5Sum", "SHA1Sum", "SHA256Sum"]:
        for entry in release.get(hn, []):
            existing_names[os.path.splitext(entry["name"])[0]] = entry["name"]

    contents_files = set()
    if components:
        for component in components:
            for arch in arches:
                contents_files.add("%s/Contents-%s" % (component, arch))
    else:
        for arch in arches:
            contents_files.add("Contents-%s" % (arch,))

    for fn in contents_files:
        if fn in existing_names:
            url = "%s/%s/%s" % (dists_url, name, fn)
            yield url


def contents_urls_from_sourceslist(sl, arch, load_url):
    # TODO(jelmer): Verify signatures, etc.
    arches = [arch, "all"]
    for source in sl.list:
        yield from contents_urls_from_sources_entry(source, arches, load_url)


def _unwrap(f, ext):
    if ext == ".gz":
        import gzip

        return gzip.GzipFile(fileobj=f)
    elif ext == ".xz":
        import lzma
        from io import BytesIO

        f = BytesIO(lzma.decompress(f.read()))
    else:
        return f


def load_direct_url(url):
    from urllib.error import HTTPError, URLError
    from urllib.request import urlopen, Request

    for ext in [".xz", ".gz", ""]:
        try:
            request = Request(url + ext, headers={"User-Agent": USER_AGENT})
            response = urlopen(request)
        except HTTPError as e:
            if e.code == 404:
                continue
            raise AptFileAccessError(
                'Unable to access apt URL %s: %s' % (url + ext, e)) from e
        except URLError as e:
            raise AptFileAccessError(
                'Unable to access apt URL %s: %s' % (url + ext, e)) from e
        break
    else:
        raise FileNotFoundError(url)
    return _unwrap(response, ext)


def load_url_with_cache(url, cache_dirs):
    for cache_dir in cache_dirs:
        with suppress(FileNotFoundError):
            return load_apt_cache_file(url, cache_dir)
    return load_direct_url(url)


def load_apt_cache_file(url, cache_dir):
    fn = apt_pkg.uri_to_filename(url)
    for ext in [".xz", ".gz", ".lz4", ""]:
        p = os.path.join(cache_dir, fn + ext)
        if not os.path.exists(p):
            continue
        # return os.popen('/usr/lib/apt/apt-helper cat-file %s' % p)
        logging.debug("Loading cached contents file %s", p)
        if ext == ".lz4":
            import lz4.frame

            return lz4.frame.open(p, mode="rb")
        try:
            f = open(p, "rb")   # noqa: SIM115
        except PermissionError as e:
            logging.warning('Unable to open %s: %s', p, e)
            raise FileNotFoundError(url) from e
        return _unwrap(f, ext)
    raise FileNotFoundError(url)


class AptFileFileSearcher(FileSearcher):

    CACHE_IS_EMPTY_PATH = '/usr/share/apt-file/is-cache-empty'

    def __init__(self, session: Session):
        self.session = session

    @classmethod
    def has_cache(cls, session: Session) -> bool:
        if not os.path.exists(session.external_path(cls.CACHE_IS_EMPTY_PATH)):
            return False
        try:
            session.check_call([cls.CACHE_IS_EMPTY_PATH])
        except subprocess.CalledProcessError as e:
            if e.returncode == 1:
                return True
            raise
        else:
            return False

    @classmethod
    def from_session(cls, session):
        logging.debug('Using apt-file to search apt contents')
        if not os.path.exists(session.external_path(cls.CACHE_IS_EMPTY_PATH)):
            from .apt import AptManager
            AptManager.from_session(session).install(['apt-file'])
        if not cls.has_cache(session):
            session.check_call(['apt-file', 'update'], user='root')
        return cls(session)

    async def search_files(self, path, regex=False, case_insensitive=False):
        args = []
        if regex:
            args.append('-x')
        else:
            args.append('-F')
        if case_insensitive:
            args.append('-i')
        args.append(path)
        process = await asyncio.create_subprocess_exec(
            '/usr/bin/apt-file', 'search', *args,
            stdout=asyncio.subprocess.PIPE)
        (output, error) = await process.communicate(input=None)
        if process.returncode == 1:
            # No results
            return
        elif process.returncode == 3:
            raise Exception('apt-file cache is empty')
        elif process.returncode != 0:
            raise Exception(
                 "unexpected return code %r" % process.returncode)

        for line in output.splitlines(False):
            pkg, path = line.split(b': ')
            yield pkg.decode('utf-8')


def get_apt_contents_file_searcher(session):
    if AptFileFileSearcher.has_cache(session):
        return AptFileFileSearcher.from_session(session)

    return RemoteContentsFileSearcher.from_session(session)


class RemoteContentsFileSearcher(FileSearcher):
    def __init__(self):
        self._db = {}

    @classmethod
    def from_session(cls, session):
        logging.info("Loading apt contents information")

        self = cls()
        self.load_from_session(session)
        return self

    def load_local(self):
        # TODO(jelmer): what about sources.list.d?
        from aptsources.sourceslist import SourcesList

        sl = SourcesList()
        sl.load("/etc/apt/sources.list")

        from .build import get_build_architecture

        cache_dirs = set(["/var/lib/apt/lists"])

        def load_url(url):
            return load_url_with_cache(url, cache_dirs)

        urls = list(
            contents_urls_from_sourceslist(
                sl, get_build_architecture(), load_url)
        )
        self._load_urls(urls, cache_dirs, load_url)

    def load_from_session(self, session):
        # TODO(jelmer): what about sources.list.d?
        from aptsources.sourceslist import SourcesList

        sl = SourcesList()
        sl.load(os.path.join(session.location, "etc/apt/sources.list"))

        from .build import get_build_architecture

        cache_dirs = set(
            [
                os.path.join(session.location, "var/lib/apt/lists"),
                "/var/lib/apt/lists",
            ]
        )

        def load_url(url):
            return load_url_with_cache(url, cache_dirs)

        urls = list(
            contents_urls_from_sourceslist(
                sl, get_build_architecture(), load_url))
        self._load_urls(urls, cache_dirs, load_url)

    def _load_urls(self, urls, cache_dirs, load_url):
        for url in urls:
            try:
                f = load_url(url)
                self.load_file(f, url)
            except ConnectionResetError:
                logging.warning("Connection reset error retrieving %s", url)
                # TODO(jelmer): Retry?
            except ContentsFileNotFound:
                logging.warning("Unable to fetch contents file %s", url)

    def __setitem__(self, path, package):
        self._db[path] = package

    async def search_files(self, path, regex=False, case_insensitive=False):
        path = path.lstrip("/").encode("utf-8", "surrogateescape")
        if case_insensitive and not regex:
            regex = True
            path = re.escape(path)
        if regex:
            flags = 0
            if case_insensitive:
                flags |= re.I
            c = re.compile(path, flags=flags)
            ret = []
            for p, rest in self._db.items():
                if c.match(p):
                    pkg = rest.split(b"/")[-1]
                    ret.append((p, pkg.decode("utf-8")))
            for _p, pkg in sorted(ret):
                yield pkg
        else:
            with suppress(KeyError):
                yield self._db[path].split(b"/")[-1].decode("utf-8")

    def load_file(self, f, url):
        start_time = datetime.now()
        for path, rest in read_contents_file(f.readlines()):
            self[path] = rest
        logging.debug("Read %s in %s", url, datetime.now() - start_time)


class GeneratedFileSearcher(FileSearcher):
    def __init__(self, db):
        self._db = db

    @classmethod
    def from_path(cls, path):
        self = cls({})
        self.load_from_path(path)
        return self

    def load_from_path(self, path):
        with open(path, "r") as f:
            for line in f:
                (path, pkg) = line.strip().split(None, 1)
                self._db.append(path, pkg)

    async def search_files(
            self, path: str, regex: bool = False,
            case_insensitive: bool = False):
        for p, pkg in self._db:
            if regex:
                flags = 0
                if case_insensitive:
                    flags |= re.I
                if re.match(path, p, flags=flags):
                    yield pkg
            elif case_insensitive:
                if path.lower() == p.lower():
                    yield pkg
            else:
                if path == p:
                    yield pkg


# TODO(jelmer): read from a file
GENERATED_FILE_SEARCHER = GeneratedFileSearcher(
    [
        ("/etc/locale.gen", "locales"),
        # Alternative
        ("/usr/bin/rst2html", "python3-docutils"),
        # aclocal is a symlink to aclocal-1.XY
        ("/usr/bin/aclocal", "automake"),
        ("/usr/bin/automake", "automake"),
        # maven lives in /usr/share
        ("/usr/bin/mvn", "maven"),
    ]
)


async def get_packages_for_paths(
    paths: List[str],
    searchers: List[FileSearcher],
    regex: bool = False,
    case_insensitive: bool = False,
) -> List[str]:
    candidates: List[str] = list()
    # TODO(jelmer): Combine these, perhaps by creating one gigantic regex?
    for path in paths:
        for searcher in searchers:
            async for pkg in searcher.search_files(
                path, regex=regex, case_insensitive=case_insensitive
            ):
                if pkg not in candidates:
                    candidates.append(pkg)
    return candidates


def main(argv):
    import argparse
    from ..session.plain import PlainSession

    parser = argparse.ArgumentParser()
    parser.add_argument(
        "path", help="Path to search for.", type=str, nargs="*")
    parser.add_argument(
        "--regex", "-x", help="Search for regex.", action="store_true")
    parser.add_argument("--debug", action="store_true")
    args = parser.parse_args()

    if args.debug:
        logging.basicConfig(level=logging.DEBUG)
    else:
        logging.basicConfig(level=logging.INFO)

    with PlainSession() as session:
        main_searcher = get_apt_contents_file_searcher(session)
        searchers = [main_searcher, GENERATED_FILE_SEARCHER]

        packages = asyncio.run(get_packages_for_paths(
            args.path, searchers=searchers, regex=args.regex))
        for package in packages:
            print(package)


if __name__ == "__main__":
    import sys

    sys.exit(main(sys.argv))

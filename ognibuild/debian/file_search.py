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
from datetime import datetime
from debian.deb822 import Release
import os
import re
from typing import Iterator, List, Optional, Set
import logging


from .. import USER_AGENT


class FileSearcher(object):
    def search_files(self, path: str, regex: bool = False) -> Iterator[str]:
        raise NotImplementedError(self.search_files)


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
            logging.warning('Unable to download %s or %s: %s', inrelease_url, release_url, e)
            return

    existing_names = {}
    release = Release(response.read())
    for hn in ['MD5Sum', 'SHA1Sum', 'SHA256Sum']:
        for entry in release.get(hn, []):
            existing_names[os.path.splitext(entry['name'])[0]] = entry['name']

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
    from urllib.error import HTTPError
    from urllib.request import urlopen, Request

    for ext in [".xz", ".gz", ""]:
        try:
            request = Request(
                url + ext, headers={"User-Agent": USER_AGENT})
            response = urlopen(request)
        except HTTPError as e:
            if e.status == 404:
                continue
            raise
        break
    else:
        raise FileNotFoundError(url)
    return _unwrap(response, ext)


def load_url_with_cache(url, cache_dirs):
    for cache_dir in cache_dirs:
        try:
            return load_apt_cache_file(url, cache_dir)
        except FileNotFoundError:
            pass
    return load_direct_url(url)


def load_apt_cache_file(url, cache_dir):
    fn = apt_pkg.uri_to_filename(url)
    for ext in ['.xz', '.gz', '.lz4', '']:
        p = os.path.join(cache_dir, fn + ext)
        if not os.path.exists(p):
            continue
        #return os.popen('/usr/lib/apt/apt-helper cat-file %s' % p)
        logging.debug("Loading cached contents file %s", p)
        if ext == '.lz4':
            import lz4.frame
            return lz4.frame.open(p, mode="rb")
        return _unwrap(open(p, 'rb'), ext)
    raise FileNotFoundError(url)


class AptCachedContentsFileSearcher(FileSearcher):
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
                sl, get_build_architecture(), load_url))
        self._load_urls(urls, cache_dirs, load_url)

    def load_from_session(self, session):
        # TODO(jelmer): what about sources.list.d?
        from aptsources.sourceslist import SourcesList

        sl = SourcesList()
        sl.load(os.path.join(session.location, "etc/apt/sources.list"))

        from .build import get_build_architecture

        cache_dirs = set([
            os.path.join(session.location, "var/lib/apt/lists"),
            "/var/lib/apt/lists",
        ])

        def load_url(url):
            return load_url_with_cache(url, cache_dirs)

        urls = list(
            contents_urls_from_sourceslist(sl, get_build_architecture(), load_url))
        self._load_urls(urls, cache_dirs, load_url)

    def _load_urls(self, urls, cache_dirs, load_url):
        for url in urls:
            try:
                f = load_url(url)
                self.load_file(f, url)
            except ContentsFileNotFound:
                logging.warning("Unable to fetch contents file %s", url)

    def __setitem__(self, path, package):
        self._db[path] = package

    def search_files(self, path, regex=False):
        path = path.lstrip('/').encode('utf-8', 'surrogateescape')
        if regex:
            c = re.compile(path)
            ret = []
            for p, rest in self._db.items():
                if c.match(p):
                    pkg = rest.split(b"/")[-1]
                    ret.append((p, pkg.decode('utf-8')))
            for p, pkg in sorted(ret):
                yield pkg
        else:
            try:
                yield self._db[path].split(b"/")[-1].decode('utf-8')
            except KeyError:
                pass

    def load_file(self, f, url):
        start_time = datetime.now()
        for path, rest in read_contents_file(f.readlines()):
            self[path] = rest
        logging.debug('Read %s in %s', url, datetime.now() - start_time)


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
        logging.debug("No packages found that contain %r", paths)
        return None
    if len(candidates) > 1:
        logging.warning(
            "More than 1 packages found that contain %r: %r", path, candidates
        )
        # Euhr. Pick the one with the shortest name?
        return sorted(candidates, key=len)[0]
    else:
        return candidates.pop()


def main(argv):
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('path', help='Path to search for.', type=str, nargs='*')
    parser.add_argument('--regex', '-x', help='Search for regex.', action='store_true')
    parser.add_argument('--debug', action='store_true')
    args = parser.parse_args()

    if args.debug:
        logging.basicConfig(level=logging.DEBUG)
    else:
        logging.basicConfig(level=logging.INFO)

    main_searcher = AptCachedContentsFileSearcher()
    main_searcher.load_local()
    searchers = [main_searcher, GENERATED_FILE_SEARCHER]

    package = get_package_for_paths(args.path, searchers=searchers, regex=args.regex)
    print(package)


if __name__ == '__main__':
    import sys
    sys.exit(main(sys.argv))

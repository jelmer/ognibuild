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
from typing import List, Optional

import os
from buildlog_consultant.apt import (
    find_apt_get_failure,
)

from .. import DetailedFailure, UnidentifiedError
from ..session import Session, run_with_tee, get_user
from .file_search import FileSearcher, AptCachedContentsFileSearcher, GENERATED_FILE_SEARCHER, get_packages_for_paths


def run_apt(session: Session, args: List[str], prefix: Optional[List[str]] = None) -> None:
    """Run apt."""
    if prefix is None:
        prefix = []
    args = prefix = ["apt", "-y"] + args
    retcode, lines = run_with_tee(session, args, cwd="/", user="root")
    if retcode == 0:
        return
    match, error = find_apt_get_failure(lines)
    if error is not None:
        raise DetailedFailure(retcode, args, error)
    while lines and lines[-1] == "":
        lines.pop(-1)
    raise UnidentifiedError(retcode, args, lines, secondary=match)


class AptManager(object):

    session: Session
    _searchers: Optional[List[FileSearcher]]

    def __init__(self, session, prefix=None):
        self.session = session
        self._apt_cache = None
        self._searchers = None
        if prefix is None:
            prefix = []
        self.prefix = prefix

    @classmethod
    def from_session(cls, session):
        if get_user(session) != "root":
            prefix = ["sudo"]
        else:
            prefix = []
        return cls(session, prefix=prefix)

    def searchers(self):
        if self._searchers is None:
            self._searchers = [
                AptCachedContentsFileSearcher.from_session(self.session),
                GENERATED_FILE_SEARCHER,
            ]
        return self._searchers

    def package_exists(self, package):
        if self._apt_cache is None:
            import apt

            self._apt_cache = apt.Cache(rootdir=self.session.location)
        return package in self._apt_cache

    def get_packages_for_paths(self, paths, regex=False):
        logging.debug("Searching for packages containing %r", paths)
        # TODO(jelmer): Make sure we use whatever is configured in self.session
        return get_packages_for_paths(paths, self.searchers(), regex=regex)

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
            run_apt(self.session, ["install"] + packages, prefix=self.prefix)

    def satisfy(self, deps: List[str]) -> None:
        run_apt(self.session, ["satisfy"] + deps, prefix=self.prefix)

    def satisfy_command(self, deps: List[str]) -> List[str]:
        return self.prefix + ["apt", "satisfy"] + deps

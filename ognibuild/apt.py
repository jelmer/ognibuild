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


from typing import List

import apt_pkg
import os
from buildlog_consultant.apt import (
    find_apt_get_failure,
    )

from . import DetailedFailure
from .session import Session, run_with_tee


class UnidentifiedError(Exception):

    def __init__(self, retcode, argv, lines, secondary=None):
        self.retcode = retcode
        self.argv = argv
        self.lines = lines
        self.secondary = secondary


def run_apt(session: Session, args: List[str]) -> None:
    """Run apt."""
    args = ['apt', '-y'] + args
    retcode, lines = run_with_tee(session, args, cwd='/', user='root')
    if retcode == 0:
        return
    offset, line, error = find_apt_get_failure(lines)
    if error is not None:
        raise DetailedFailure(retcode, args, error)
    if line is not None:
        raise UnidentifiedError(
            retcode, args, lines, secondary=(offset, line))
    while lines and lines[-1] == '':
        lines.pop(-1)
    raise UnidentifiedError(retcode, args, lines)


class AptManager(object):

    session: Session

    def __init__(self, session):
        self.session = session

    def missing(self, packages):
        root = getattr(self.session, 'location', '/')
        status_path = os.path.join(root, 'var/lib/dpkg/status')
        missing = set(packages)
        with apt_pkg.TagFile(status_path) as tagf:
            while missing:
                tagf.step()
                if not tagf.section:
                    break
                if tagf.section['Package'] in missing:
                    if tagf.section['Status'] == 'install ok installed':
                        missing.remove(tagf.section['Package'])
        return list(missing)

    def install(self, packages: List[str]) -> None:
        packages = self.missing(packages)
        if packages:
            run_apt(self.session, ['install'] + packages)

    def satisfy(self, deps: List[str]) -> None:
        run_apt(self.session, ['satisfy'] + deps)

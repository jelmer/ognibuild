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

import os
import stat
from typing import List


__version__ = (0, 0, 11)


USER_AGENT = "Ognibuild"


class DetailedFailure(Exception):
    def __init__(self, retcode, argv, error):
        self.retcode = retcode
        self.argv = argv
        self.error = error

    def __eq__(self, other):
        return (isinstance(other, type(self)) and
                self.retcode == other.retcode and
                self.argv == other.argv and
                self.error == other.error)


class UnidentifiedError(Exception):
    """An unidentified error."""

    def __init__(self, retcode, argv, lines, secondary=None):
        self.retcode = retcode
        self.argv = argv
        self.lines = lines
        self.secondary = secondary

    def __eq__(self, other):
        return (isinstance(other, type(self)) and
                self.retcode == other.retcode and
                self.argv == other.argv and
                self.lines == other.lines and
                self.secondary == other.secondary)

    def __repr__(self):
        return "<%s(%r, %r, ..., secondary=%r)>" % (
            type(self).__name__,
            self.retcode,
            self.argv,
            self.secondary,
        )


def shebang_binary(p):
    if not (os.stat(p).st_mode & stat.S_IEXEC):
        return None
    with open(p, "rb") as f:
        firstline = f.readline()
        if not firstline.startswith(b"#!"):
            return None
        args = firstline[2:].strip().split(b" ")
        if args[0] in (b"/usr/bin/env", b"env"):
            return os.path.basename(args[1].decode()).strip()
        return os.path.basename(args[0].decode()).strip()


class Requirement(object):

    # Name of the family of requirements - e.g. "python-package"
    family: str

    def __init__(self, family):
        self.family = family

    def met(self, session):
        raise NotImplementedError(self)


class OneOfRequirement(Requirement):

    elements: List[Requirement]

    family = 'or'

    def __init__(self, elements):
        self.elements = elements

    def met(self, session):
        for req in self.elements:
            if req.met(session):
                return True
        return False

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.elements)


class UpstreamOutput(object):
    def __init__(self, family):
        self.family = family

    def get_declared_dependencies(self):
        raise NotImplementedError(self.get_declared_dependencies)

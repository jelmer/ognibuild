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

from . import UpstreamOutput


class BinaryOutput(UpstreamOutput):
    def __init__(self, name) -> None:
        super().__init__("binary")
        self.name = name

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.name!r})"

    def __str__(self) -> str:
        return "binary: %s" % self.name


class PythonPackageOutput(UpstreamOutput):
    def __init__(self, name, python_version=None) -> None:
        super().__init__("python-package")
        self.name = name
        self.python_version = python_version

    def __str__(self) -> str:
        return "python package: %s" % self.name

    def __repr__(self) -> str:
        return "{}({!r}, python_version={!r})".format(
            type(self).__name__,
            self.name,
            self.python_version,
        )


class RPackageOutput(UpstreamOutput):
    def __init__(self, name) -> None:
        super().__init__("r-package")
        self.name = name

    def __str__(self) -> str:
        return "R package: %s" % self.name

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.name!r})"

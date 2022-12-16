#!/usr/bin/python
# Copyright (C) 2022 Jelmer Vernooij <jelmer@jelmer.uk>
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

from ognibuild.buildlog import PROBLEM_CONVERTERS

from buildlog_consultant import (
    problem_clses,
    __version__ as buildlog_consultant_version,
)

from unittest import TestCase


class TestProblemsExists(TestCase):

    def test_exist(self):
        for entry in PROBLEM_CONVERTERS:
            if len(entry) == 2:
                problem_kind, fn = entry  # type: ignore
                min_version = None
            elif len(entry) == 3:
                problem_kind, fn, min_version = entry  # type: ignore
            else:
                raise TypeError(entry)
            if min_version is not None:
                min_version_tuple = tuple(
                    [int(x) for x in min_version.split('.')])
                if buildlog_consultant_version < min_version_tuple:
                    continue
            self.assertTrue(
                problem_kind in problem_clses,
                f"{problem_kind} does not exist in known "
                "buildlog-consultant problem kinds")

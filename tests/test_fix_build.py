#!/usr/bin/python
# Copyright (C) 2024 Jelmer Vernooij <jelmer@jelmer.uk>
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

from unittest import TestCase

from ognibuild.fix_build import BuildFixer, iterate_with_build_fixers


class FixingBuildFixer(BuildFixer):

    def can_fix(self, p):
        return True

    def fix(self, p, phase):
        return True


class IncompatibleBuildFixer(BuildFixer):

    def can_fix(self, p):
        return False

    def fix(self, p, phase):
        return None


class NotFixingBuildFixer(BuildFixer):

    def can_fix(self, p):
        return True

    def fix(self, p, phase):
        return None


class TestIterateWithBuildFixers(TestCase):

    def test_no_problem(self):
        self.assertIs(
            iterate_with_build_fixers(
                [FixingBuildFixer(), IncompatibleBuildFixer()],
                ["a", "b"],
                lambda: None),
                None)

    def test_fixable_problem(self):
        self.assertIs(
            iterate_with_build_fixers(
                [FixingBuildFixer(), IncompatibleBuildFixer()],
                ["a", "b"],
                ["problem", None].pop),
                None)

    def test_not_fixable_problem(self):
        self.assertEqual(
            iterate_with_build_fixers(
                [NotFixingBuildFixer(), IncompatibleBuildFixer()],
                ["a", "b"],
                ["problem", "problem"].pop),
            "problem")

    def test_no_fixers(self):
        self.assertEqual(
            iterate_with_build_fixers(
                [],
                ["a", "b"],
                ["problem", "problem"].pop),
            "problem")

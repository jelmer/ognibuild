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

from unittest import TestCase


from ognibuild.resolver.apt import get_possible_python3_paths_for_python_object


class TestPython3Paths(TestCase):

    def test_paths(self):
        self.assertEqual([
            '/usr/lib/python3/dist\\-packages/dulwich/__init__\\.py',
            '/usr/lib/python3/dist\\-packages/dulwich\\.py',
            '/usr/lib/python3\\.[0-9]+/'
            'lib\\-dynload/dulwich.cpython\\-.*\\.so',
            '/usr/lib/python3\\.[0-9]+/dulwich\\.py',
            '/usr/lib/python3\\.[0-9]+/dulwich/__init__\\.py'],
            get_possible_python3_paths_for_python_object('dulwich'))
        self.assertEqual([
            '/usr/lib/python3/dist\\-packages/cleo/foo/__init__\\.py',
            '/usr/lib/python3/dist\\-packages/cleo/foo\\.py',
            '/usr/lib/python3\\.[0-9]+/'
            'lib\\-dynload/cleo/foo.cpython\\-.*\\.so',
            '/usr/lib/python3\\.[0-9]+/cleo/foo\\.py',
            '/usr/lib/python3\\.[0-9]+/cleo/foo/__init__\\.py',
            '/usr/lib/python3/dist\\-packages/cleo/__init__\\.py',
            '/usr/lib/python3/dist\\-packages/cleo\\.py',
            '/usr/lib/python3\\.[0-9]+/lib\\-dynload/cleo.cpython\\-.*\\.so',
            '/usr/lib/python3\\.[0-9]+/cleo\\.py',
            '/usr/lib/python3\\.[0-9]+/cleo/__init__\\.py'],
            get_possible_python3_paths_for_python_object('cleo.foo'))

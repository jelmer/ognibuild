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

import os
import sys
import tempfile
from unittest import TestCase

from ognibuild.logs import (
    copy_output,
    redirect_output,
    rotate_logfile,
    DirectoryLogManager,
)


class TestCopyOutput(TestCase):

    def test_no_tee(self):
        with tempfile.TemporaryDirectory() as td:
            p = os.path.join(td, 'foo.log')
            with copy_output(p, tee=False):
                sys.stdout.write('lala\n')
                sys.stdout.flush()
            with open(p, 'r') as f:
                self.assertEqual('lala\n', f.read())

    def test_tee(self):
        with tempfile.TemporaryDirectory() as td:
            p = os.path.join(td, 'foo.log')
            with copy_output(p, tee=True):
                sys.stdout.write('lala\n')
                sys.stdout.flush()
            with open(p, 'r') as f:
                self.assertEqual('lala\n', f.read())


class TestRedirectOutput(TestCase):

    def test_simple(self):
        with tempfile.TemporaryDirectory() as td:
            p = os.path.join(td, 'foo.log')
            with open(p, 'w') as f, redirect_output(f):
                sys.stdout.write('lala\n')
                sys.stdout.flush()
            with open(p, 'r') as f:
                self.assertEqual('lala\n', f.read())


class TestRotateLogfile(TestCase):

    def test_does_not_exist(self):
        with tempfile.TemporaryDirectory() as td:
            p = os.path.join(td, 'foo.log')
            rotate_logfile(p)
            self.assertEqual([], os.listdir(td))

    def test_simple(self):
        with tempfile.TemporaryDirectory() as td:
            p = os.path.join(td, 'foo.log')
            with open(p, 'w') as f:
                f.write('contents\n')
            rotate_logfile(p)
            self.assertEqual(['foo.log.1'], os.listdir(td))


class TestLogManager(TestCase):

    def test_simple(self):
        with tempfile.TemporaryDirectory() as td:
            p = os.path.join(td, 'foo.log')
            lm = DirectoryLogManager(p, mode='redirect')

            def writesomething():
                sys.stdout.write('foo\n')
                sys.stdout.flush()
            fn = lm.wrap(writesomething)
            fn()
            with open(p, 'r') as f:
                self.assertEqual('foo\n', f.read())

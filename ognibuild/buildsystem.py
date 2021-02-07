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
import os

from .apt import AptManager
from .fix_build import run_with_build_fixer


class NoBuildToolsFound(Exception):
    """No supported build tools were found."""


class BuildSystem(object):
    """A particular buildsystem."""

    def __init__(self, session):
        self.session = session

    def dist(self):
        raise NotImplementedError(self.dist)

    def test(self):
        raise NotImplementedError(self.test)

    def build(self):
        raise NotImplementedError(self.build)

    def clean(self):
        raise NotImplementedError(self.clean)

    def install(self):
        raise NotImplementedError(self.install)


class Pear(BuildSystem):

    def dist(self):
        apt = AptManager(self.session)
        apt.install(['php-pear'])
        run_with_build_fixer(self.session, ['pear', 'package'])

    def test(self):
        apt = AptManager(self.session)
        apt.install(['php-pear'])
        run_with_build_fixer(self.session, ['pear', 'run-tests'])

    def build(self):
        apt = AptManager(self.session)
        apt.install(['php-pear'])
        run_with_build_fixer(self.session, ['pear', 'build'])

    def clean(self):
        apt = AptManager(self.session)
        apt.install(['php-pear'])
        # TODO

    def install(self):
        apt = AptManager(self.session)
        apt.install(['php-pear'])
        run_with_build_fixer(self.session, ['pear', 'install'])


def detect_buildsystems(session):
    """Detect build systems."""
    if os.path.exists('package.xml'):
        logging.info('Found package.xml, assuming pear package.')
        yield Pear(session)

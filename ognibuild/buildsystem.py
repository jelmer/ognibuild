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

from . import shebang_binary
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


class SetupPy(BuildSystem):

    def prereqs(self):
        apt = AptManager(self.session)
        apt.install(['python3', 'python3-pip'])
        with open('setup.py', 'r') as f:
            setup_py_contents = f.read()
        try:
            with open('setup.cfg', 'r') as f:
                setup_cfg_contents = f.read()
        except FileNotFoundError:
            setup_cfg_contents = ''
        if 'setuptools' in setup_py_contents:
            logging.info('Reference to setuptools found, installing.')
            apt.install(['python3-setuptools'])
        if ('setuptools_scm' in setup_py_contents or
                'setuptools_scm' in setup_cfg_contents):
            logging.info('Reference to setuptools-scm found, installing.')
            apt.install(['python3-setuptools-scm', 'git', 'mercurial'])

        # TODO(jelmer): Install setup_requires

    def test(self):
        self.prereqs()
        self._run_setup(['test'])

    def dist(self):
        self.prereqs()
        self._run_setup(['sdist'])

    def clean(self):
        self.prereqs()
        self._run_setup(['clean'])

    def install(self):
        self.prereqs()
        self._run_setup(['install'])

    def _run_setup(self, args):
        apt = AptManager(self.session)
        interpreter = shebang_binary('setup.py')
        if interpreter is not None:
            if interpreter == 'python3':
                apt.install(['python3'])
            elif interpreter == 'python2':
                apt.install(['python2'])
            elif interpreter == 'python':
                apt.install(['python'])
            else:
                raise ValueError('Unknown interpreter %r' % interpreter)
            apt.install(['python2', 'python3'])
            run_with_build_fixer(
                self.session, ['./setup.py'] + args)
        else:
            # Just assume it's Python 3
            apt.install(['python3'])
            run_with_build_fixer(
                self.session, ['python3', './setup.py'] + args)


class PyProject(BuildSystem):

    def load_toml(self):
        import toml
        with open('pyproject.toml', 'r') as pf:
            return toml.load(pf)

    def dist(self):
        apt = AptManager(self.session)
        pyproject = self.load_toml()
        if 'poetry' in pyproject.get('tool', []):
            logging.info(
                'Found pyproject.toml with poetry section, '
                'assuming poetry project.')
            apt.install(['python3-venv', 'python3-pip'])
            self.session.check_call(['pip3', 'install', 'poetry'], user='root')
            self.session.check_call(['poetry', 'build', '-f', 'sdist'])
            return
        raise AssertionError('no supported section in pyproject.toml')


def detect_buildsystems(session):
    """Detect build systems."""
    if os.path.exists('package.xml'):
        logging.info('Found package.xml, assuming pear package.')
        yield Pear(session)

    if os.path.exists('setup.py'):
        logging.info('Found setup.py, assuming python project.')
        yield SetupPy(session)

    if os.path.exists('pyproject.toml'):
        logging.info('Found pyproject.toml, assuming python project.')
        yield PyProject(session)

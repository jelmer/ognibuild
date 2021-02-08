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
import re

from . import shebang_binary, UpstreamPackage
from .apt import UnidentifiedError
from .fix_build import run_with_build_fixer


class NoBuildToolsFound(Exception):
    """No supported build tools were found."""


class BuildSystem(object):
    """A particular buildsystem."""

    def __init__(self, session):
        self.session = session

    def dist(self, resolver):
        raise NotImplementedError(self.dist)

    def test(self, resolver):
        raise NotImplementedError(self.test)

    def build(self, resolver):
        raise NotImplementedError(self.build)

    def clean(self, resolver):
        raise NotImplementedError(self.clean)

    def install(self, resolver):
        raise NotImplementedError(self.install)


class Pear(BuildSystem):

    def setup(self, resolver):
        resolver.install([UpstreamPackage('binary', 'pear')])

    def dist(self, resolver):
        self.setup(resolver)
        run_with_build_fixer(self.session, ['pear', 'package'])

    def test(self, resolver):
        self.setup()
        run_with_build_fixer(self.session, ["pear", "run-tests"])

    def build(self, resolver):
        self.setup(resolver)
        run_with_build_fixer(self.session, ['pear', 'build'])

    def clean(self, resolver):
        self.setup(resolver)
        # TODO

    def install(self, resolver):
        self.setup(resolver)
        run_with_build_fixer(self.session, ['pear', 'install'])


class SetupPy(BuildSystem):

    def setup(self, resolver):
        resolver.install([
            UpstreamPackage('python3', 'pip'),
            UpstreamPackage('binary', 'python3'),
            ])
        with open('setup.py', 'r') as f:
            setup_py_contents = f.read()
        try:
            with open("setup.cfg", "r") as f:
                setup_cfg_contents = f.read()
        except FileNotFoundError:
            setup_cfg_contents = ''
        if 'setuptools' in setup_py_contents:
            logging.info('Reference to setuptools found, installing.')
            resolver.install([UpstreamPackage('python3', 'setuptools')])
        if ('setuptools_scm' in setup_py_contents or
                'setuptools_scm' in setup_cfg_contents):
            logging.info('Reference to setuptools-scm found, installing.')
            resolver.install([
                UpstreamPackage('python3', 'setuptools-scm'),
                UpstreamPackage('binary', 'git'),
                UpstreamPackage('binary', 'mercurial'),
                ])

        # TODO(jelmer): Install setup_requires

    def test(self, resolver):
        self.setup(resolver)
        self._run_setup(resolver, ['test'])

    def dist(self, resolver):
        self.setup(resolver)
        self._run_setup(resolver, ['sdist'])

    def clean(self, resolver):
        self.setup(resolver)
        self._run_setup(resolver, ['clean'])

    def install(self, resolver):
        self.setup(resolver)
        self._run_setup(resolver, ['install'])

    def _run_setup(self, resolver, args):
        interpreter = shebang_binary('setup.py')
        if interpreter is not None:
            if interpreter in ('python3', 'python2', 'python'):
                resolver.install([UpstreamPackage('binary', interpreter)])
            else:
                raise ValueError('Unknown interpreter %r' % interpreter)
            run_with_build_fixer(
                self.session, ['./setup.py'] + args)
        else:
            # Just assume it's Python 3
            resolver.install([UpstreamPackage('binary', 'python3')])
            run_with_build_fixer(
                self.session, ['python3', './setup.py'] + args)


class PyProject(BuildSystem):
    def load_toml(self):
        import toml

        with open("pyproject.toml", "r") as pf:
            return toml.load(pf)

    def dist(self, resolver):
        pyproject = self.load_toml()
        if "poetry" in pyproject.get("tool", []):
            logging.info(
                'Found pyproject.toml with poetry section, '
                'assuming poetry project.')
            resolver.install([
                UpstreamPackage('python3', 'venv'),
                UpstreamPackage('python3', 'pip'),
                ])
            self.session.check_call(['pip3', 'install', 'poetry'], user='root')
            self.session.check_call(['poetry', 'build', '-f', 'sdist'])
            return
        raise AssertionError("no supported section in pyproject.toml")


class SetupCfg(BuildSystem):

    def setup(self, resolver):
        resolver.install([
            UpstreamPackage('python3', 'pep517'),
            UpstreamPackage('python3', 'pip'),
            ])

    def dist(self, resolver):
        self.setup(resolver)
        self.session.check_call(['python3', '-m', 'pep517.build', '-s', '.'])


class NpmPackage(BuildSystem):

    def setup(self, resolver):
        resolver.install([UpstreamPackage('binary', 'npm')])

    def dist(self, resolver):
        self.setup(resolver)
        run_with_build_fixer(self.session, ['npm', 'pack'])


class Waf(BuildSystem):

    def setup(self, resolver):
        resolver.install([UpstreamPackage('binary', 'python3')])

    def dist(self, resolver):
        self.setup(resolver)
        run_with_build_fixer(self.session, ['./waf', 'dist'])


class Gem(BuildSystem):

    def setup(self, resolver):
        resolver.install([UpstreamPackage('binary', 'gem2deb')])

    def dist(self, resolver):
        self.setup(resolver)
        gemfiles = [entry.name for entry in self.session.scandir('.')
                    if entry.name.endswith('.gem')]
        if len(gemfiles) > 1:
            logging.warning("More than one gemfile. Trying the first?")
        run_with_build_fixer(self.session, ["gem2tgz", gemfiles[0]])


class DistInkt(BuildSystem):

    def setup(self, resolver):
        resolver.install([
            UpstreamPackage('perl', 'Dist::Inkt'),
            ])

    def dist(self, resolver):
        self.setup(resolver)
        with open('dist.ini', 'rb') as f:
            for line in f:
                if not line.startswith(b";;"):
                    continue
                try:
                    (key, value) = line[2:].split(b"=", 1)
                except ValueError:
                    continue
                if key.strip() == b"class" and value.strip().startswith(b"'Dist::Inkt"):
                    logging.info(
                        "Found Dist::Inkt section in dist.ini, " "assuming distinkt."
                    )
                    # TODO(jelmer): install via apt if possible
                    self.session.check_call(
                        ["cpan", "install", value.decode().strip("'")], user="root"
                    )
                    run_with_build_fixer(self.session, ["distinkt-dist"])
                    return
        # Default to invoking Dist::Zilla
        logging.info('Found dist.ini, assuming dist-zilla.')
        resolver.install([UpstreamPackage('perl', 'Dist::Zilla')])
        run_with_build_fixer(self.session, ['dzil', 'build', '--in', '..'])


class Make(BuildSystem):

    def setup(self, resolver):
        if self.session.exists('Makefile.PL') and not self.session.exists('Makefile'):
            resolver.install([UpstreamPackage('binary', 'perl')])
            run_with_build_fixer(self.session, ['perl', 'Makefile.PL'])

        if not self.session.exists('Makefile') and not self.session.exists('configure'):
            if self.session.exists('autogen.sh'):
                if shebang_binary('autogen.sh') is None:
                    run_with_build_fixer(
                        self.session, ['/bin/sh', './autogen.sh'])
                try:
                    run_with_build_fixer(self.session, ["./autogen.sh"])
                except UnidentifiedError as e:
                    if (
                        "Gnulib not yet bootstrapped; "
                        "run ./bootstrap instead.\n" in e.lines
                    ):
                        run_with_build_fixer(self.session, ["./bootstrap"])
                        run_with_build_fixer(self.session, ["./autogen.sh"])
                    else:
                        raise

            elif self.session.exists("configure.ac") or self.session.exists(
                "configure.in"
            ):
                apt.install(
                    ["autoconf", "automake", "gettext", "libtool", "gnu-standards"]
                )
                run_with_build_fixer(self.session, ["autoreconf", "-i"])

        if not self.session.exists("Makefile") and self.session.exists("configure"):
            self.session.check_call(["./configure"])

    def dist(self, resolver):
        self.setup(resolver)
        resolver.install([UpstreamPackage('binary', 'make')])
        try:
            run_with_build_fixer(self.session, ["make", "dist"])
        except UnidentifiedError as e:
            if "make: *** No rule to make target 'dist'.  Stop.\n" in e.lines:
                pass
            elif "make[1]: *** No rule to make target 'dist'. Stop.\n" in e.lines:
                pass
            elif (
                "Reconfigure the source tree "
                "(via './config' or 'perl Configure'), please.\n"
            ) in e.lines:
                run_with_build_fixer(self.session, ["./config"])
                run_with_build_fixer(self.session, ["make", "dist"])
            elif (
                "Please try running 'make manifest' and then run "
                "'make dist' again.\n" in e.lines
            ):
                run_with_build_fixer(self.session, ["make", "manifest"])
                run_with_build_fixer(self.session, ["make", "dist"])
            elif "Please run ./configure first\n" in e.lines:
                run_with_build_fixer(self.session, ["./configure"])
                run_with_build_fixer(self.session, ["make", "dist"])
            elif any(
                [
                    re.match(
                        r"Makefile:[0-9]+: \*\*\* Missing \'Make.inc\' "
                        r"Run \'./configure \[options\]\' and retry.  Stop.\n",
                        line,
                    )
                    for line in e.lines
                ]
            ):
                run_with_build_fixer(self.session, ["./configure"])
                run_with_build_fixer(self.session, ["make", "dist"])
            elif any(
                [
                    re.match(
                        r"Problem opening MANIFEST: No such file or directory "
                        r"at .* line [0-9]+\.",
                        line,
                    )
                    for line in e.lines
                ]
            ):
                run_with_build_fixer(self.session, ["make", "manifest"])
                run_with_build_fixer(self.session, ["make", "dist"])
            else:
                raise
        else:
            return


def detect_buildsystems(session):
    """Detect build systems."""
    if session.exists("package.xml"):
        logging.info("Found package.xml, assuming pear package.")
        yield Pear(session)

    if session.exists("setup.py"):
        logging.info("Found setup.py, assuming python project.")
        yield SetupPy(session)

    if session.exists("pyproject.toml"):
        logging.info("Found pyproject.toml, assuming python project.")
        yield PyProject(session)

    if session.exists("setup.cfg"):
        logging.info("Found setup.cfg, assuming python project.")
        yield SetupCfg(session)

    if session.exists("package.json"):
        logging.info("Found package.json, assuming node package.")
        yield NpmPackage(session)

    if session.exists("waf"):
        logging.info("Found waf, assuming waf package.")
        yield Waf(session)

    gemfiles = [
        entry.name for entry in session.scandir(".") if entry.name.endswith(".gem")
    ]
    if gemfiles:
        yield Gem(session)

    if session.exists("dist.ini") and not session.exists("Makefile.PL"):
        yield DistInkt(session)

    if any(
        [
            session.exists(p)
            for p in [
                "Makefile",
                "Makefile.PL",
                "autogen.sh",
                "configure.ac",
                "configure.in",
            ]
        ]
    ):
        yield Make(session)

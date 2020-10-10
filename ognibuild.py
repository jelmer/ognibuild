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
import subprocess
import sys
from typing import List


DEFAULT_PYTHON = 'python3'


class UnidentifiedError(Exception):

    def __init__(self, retcode, argv, lines):
        self.retcode = retcode
        self.argv = argv
        self.lines = lines


class NoBuildToolsFound(Exception):
    """No supported build tools were found."""


def shebang_binary(p):
    if not (os.stat(p).st_mode & stat.S_IEXEC):
        return None
    with open(p, 'rb') as f:
        firstline = f.readline()
        if not firstline.startswith(b'#!'):
            return None
        args = firstline[2:].split(b' ')
        if args[0] in (b'/usr/bin/env', b'env'):
            return os.path.basename(args[1].decode())
        return os.path.basename(args[0].decode())


def note(m):
    sys.stdout.write('%s\n' % m)


def warning(m):
    sys.stderr.write('WARNING: %s\n' % m)


def run_with_tee(session, args: List[str], **kwargs):
    p = session.Popen(
        args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, **kwargs)
    contents = []
    while p.poll() is None:
        line = p.stdout.readline()
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
        contents.append(line.decode('utf-8', 'surrogateescape'))
    return p.returncode, contents


def run_apt(session, args: List[str]) -> None:
    args = ['apt', '-y'] + args
    retcode, lines = run_with_tee(session, args, cwd='/', user='root')
    if retcode == 0:
        return
    raise UnidentifiedError(retcode, args, lines)


def apt_install(session, packages: List[str]) -> None:
    run_apt(session, ['install'] + packages)


def run_with_build_fixer(session, args):
    session.check_call(args)


def run_dist(session):
    # TODO(jelmer): Check $PATH rather than hardcoding?
    if not os.path.exists('/usr/bin/git'):
        apt_install(session, ['git'])

    # Some things want to write to the user's home directory,
    # e.g. pip caches in ~/.cache
    session.create_home()

    if os.path.exists('package.xml'):
        apt_install(session, ['php-pear', 'php-horde-core'])
        note('Found package.xml, assuming pear package.')
        session.check_call(['pear', 'package'])
        return

    if os.path.exists('pyproject.toml'):
        import toml
        with open('pyproject.toml', 'r') as pf:
            pyproject = toml.load(pf)
        if 'poetry' in pyproject.get('tool', []):
            note('Found pyproject.toml with poetry section, '
                 'assuming poetry project.')
            apt_install(session, ['python3-venv', 'python3-pip'])
            session.check_call(['pip3', 'install', 'poetry'], user='root')
            session.check_call(['poetry', 'build', '-f', 'sdist'])
            return

    if os.path.exists('setup.py'):
        note('Found setup.py, assuming python project.')
        apt_install(session, ['python3', 'python3-pip'])
        with open('setup.py', 'r') as f:
            setup_py_contents = f.read()
        try:
            with open('setup.cfg', 'r') as f:
                setup_cfg_contents = f.read()
        except FileNotFoundError:
            setup_cfg_contents = ''
        if 'setuptools' in setup_py_contents:
            note('Reference to setuptools found, installing.')
            apt_install(session, ['python3-setuptools'])
        if ('setuptools_scm' in setup_py_contents or
                'setuptools_scm' in setup_cfg_contents):
            note('Reference to setuptools-scm found, installing.')
            apt_install(
                session, ['python3-setuptools-scm', 'git', 'mercurial'])

        # TODO(jelmer): Install setup_requires

        interpreter = shebang_binary('setup.py')
        if interpreter is not None:
            if interpreter == 'python2' or interpreter.startswith('python2.'):
                apt_install(session, [interpreter])
            elif (interpreter == 'python3' or
                    interpreter.startswith('python3.')):
                apt_install(session, [interpreter])
            else:
                apt_install(session, [DEFAULT_PYTHON])
            run_with_build_fixer(session, ['./setup.py', 'sdist'])
        else:
            # Just assume it's Python 3
            apt_install(session, ['python3'])
            run_with_build_fixer(session, ['python3', './setup.py', 'sdist'])
        return

    if os.path.exists('setup.cfg'):
        note('Found setup.cfg, assuming python project.')
        apt_install(session, ['python3-pep517', 'python3-pip'])
        session.check_call(['python3', '-m', 'pep517.build', '-s', '.'])
        return

    if os.path.exists('dist.ini') and not os.path.exists('Makefile.PL'):
        apt_install(session, ['libdist-inkt-perl'])
        with open('dist.ini', 'rb') as f:
            for line in f:
                if not line.startswith(b';;'):
                    continue
                try:
                    (key, value) = line[2:].split(b'=', 1)
                except ValueError:
                    continue
                if (key.strip() == b'class' and
                        value.strip().startswith(b"'Dist::Inkt")):
                    note('Found Dist::Inkt section in dist.ini, '
                         'assuming distinkt.')
                    # TODO(jelmer): install via apt if possible
                    session.check_call(
                        ['cpan', 'install', value.decode().strip("'")],
                        user='root')
                    run_with_build_fixer(session, ['distinkt-dist'])
                    return
        # Default to invoking Dist::Zilla
        note('Found dist.ini, assuming dist-zilla.')
        apt_install(session, ['libdist-zilla-perl'])
        run_with_build_fixer(session, ['dzil', 'build', '--in', '..'])
        return

    if os.path.exists('package.json'):
        apt_install(session, ['npm'])
        run_with_build_fixer(session, ['npm', 'pack'])
        return

    gemfiles = [name for name in os.listdir('.') if name.endswith('.gem')]
    if gemfiles:
        apt_install(session, ['gem2deb'])
        if len(gemfiles) > 1:
            warning('More than one gemfile. Trying the first?')
        run_with_build_fixer(session, ['gem2tgz', gemfiles[0]])
        return

    if os.path.exists('waf'):
        apt_install(session, ['python3'])
        run_with_build_fixer(session, ['./waf', 'dist'])
        return

    if os.path.exists('Makefile.PL') and not os.path.exists('Makefile'):
        apt_install(session, ['perl'])
        run_with_build_fixer(session, ['perl', 'Makefile.PL'])

    if not os.path.exists('Makefile') and not os.path.exists('configure'):
        if os.path.exists('autogen.sh'):
            if shebang_binary('autogen.sh') is None:
                run_with_build_fixer(session, ['/bin/sh', './autogen.sh'])
            else:
                run_with_build_fixer(session, ['./autogen.sh'])

        elif os.path.exists('configure.ac') or os.path.exists('configure.in'):
            apt_install(session, [
                'autoconf', 'automake', 'gettext', 'libtool', 'gnu-standards'])
            run_with_build_fixer(session, ['autoreconf', '-i'])

    if not os.path.exists('Makefile') and os.path.exists('configure'):
        session.check_call(['./configure'])

    if os.path.exists('Makefile'):
        apt_install(session, ['make'])
        run_with_build_fixer(session, ['make', 'dist'])

    raise NoBuildToolsFound()


class PlainSession(object):
    """Session ignoring user."""

    def create_home(self):
        pass

    def check_call(self, args):
        return subprocess.check_call(args)

    def Popen(self, args, stdout=None, stderr=None, user=None, cwd=None):
        return subprocess.Popen(
            args, stdout=stdout, stderr=stderr, cwd=cwd)


def main(argv):
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('subcommand', type=str, choices=['dist'])
    parser.add_argument(
        '--directory', '-d', type=str, help='Directory for project.',
        default='.')
    args = parser.parse_args()
    session = PlainSession()
    os.chdir(args.directory)
    try:
        if args.subcommand == 'dist':
            run_dist(session)
    except NoBuildToolsFound:
        note('No build tools found.')
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))

#!/usr/bin/python3
# Copyright (C) 2020 Jelmer Vernooij <jelmer@jelmer.uk>
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

import errno
import logging
import os
import re
import shutil
import sys
import tempfile
from typing import Optional, List, Tuple, Callable, Type

from debian.deb822 import Deb822

from breezy.errors import NotBranchError
from breezy.export import export
from breezy.tree import Tree
from breezy.workingtree import WorkingTree

from breezy.plugins.debian.repack_tarball import get_filetype

from . import DetailedFailure, shebang_binary
from .apt import AptResolver, UnidentifiedError
from .buildsystem import detect_buildsystems, NoBuildToolsFound
from .session import run_with_tee, Session
from .session.schroot import SchrootSession
from .debian.fix_build import (
    DependencyContext,
    resolve_error,
    APT_FIXERS,
    )
from buildlog_consultant.sbuild import (
    find_build_failure_description,
    Problem,
    MissingPerlModule,
    MissingCommand,
    NoSpaceOnDevice,
    )


class DistNoTarball(Exception):
    """Dist operation did not create a tarball."""


def satisfy_build_deps(session: Session, tree):
    source = Deb822(tree.get_file('debian/control'))
    deps = []
    for name in ['Build-Depends', 'Build-Depends-Indep', 'Build-Depends-Arch']:
        try:
            deps.append(source[name].strip().strip(','))
        except KeyError:
            pass
    for name in ['Build-Conflicts', 'Build-Conflicts-Indeo',
                 'Build-Conflicts-Arch']:
        try:
            deps.append('Conflicts: ' + source[name])
        except KeyError:
            pass
    deps = [
        dep.strip().strip(',')
        for dep in deps]
    apt = AptResolver(session)
    apt.satisfy(deps)


class SchrootDependencyContext(DependencyContext):

    def __init__(self, session):
        self.session = session
        self.apt = AptResolver(session)

    def add_dependency(self, package, minimum_version=None):
        # TODO(jelmer): Handle minimum_version
        self.apt.install([package])
        return True


def fix_perl_module_from_cpan(error, context):
    # TODO(jelmer): Specify -T to skip tests?
    context.session.check_call(
        ['cpan', '-i', error.module], user='root',
        env={'PERL_MM_USE_DEFAULT': '1'})
    return True


NPM_COMMAND_PACKAGES = {
    'del-cli': 'del-cli',
    }


def fix_npm_missing_command(error, context):
    try:
        package = NPM_COMMAND_PACKAGES[error.command]
    except KeyError:
        return False

    context.session.check_call(['npm', '-g', 'install', package])
    return True


GENERIC_INSTALL_FIXERS: List[
        Tuple[Type[Problem], Callable[[Problem, DependencyContext], bool]]] = [
    (MissingPerlModule, fix_perl_module_from_cpan),
    (MissingCommand, fix_npm_missing_command),
]


def run_with_build_fixer(session: Session, args: List[str]):
    logging.info('Running %r', args)
    fixed_errors = []
    while True:
        retcode, lines = run_with_tee(session, args)
        if retcode == 0:
            return
        offset, line, error = find_build_failure_description(lines)
        if error is None:
            logging.warning('Build failed with unidentified error. Giving up.')
            if line is not None:
                raise UnidentifiedError(
                    retcode, args, lines, secondary=(offset, line))
            raise UnidentifiedError(retcode, args, lines)

        logging.info('Identified error: %r', error)
        if error in fixed_errors:
            logging.warning(
                'Failed to resolve error %r, it persisted. Giving up.',
                error)
            raise DetailedFailure(retcode, args, error)
        if not resolve_error(
                error, SchrootDependencyContext(session),
                fixers=(APT_FIXERS + GENERIC_INSTALL_FIXERS)):
            logging.warning(
                'Failed to find resolution for error %r. Giving up.',
                error)
            raise DetailedFailure(retcode, args, error)
        fixed_errors.append(error)


def run_dist(session):
    apt = AptResolver(session)
    apt.install(['git'])

    # Some things want to write to the user's home directory,
    # e.g. pip caches in ~/.cache
    session.create_home()

    for buildsystem in detect_buildsystems(session):
        buildsystem.dist()
        return

    if os.path.exists('package.xml'):
        apt.install(['php-pear', 'php-horde-core'])
        logging.info('Found package.xml, assuming pear package.')
        session.check_call(['pear', 'package'])
        return

    if os.path.exists('pyproject.toml'):
        import toml
        with open('pyproject.toml', 'r') as pf:
            pyproject = toml.load(pf)
        if 'poetry' in pyproject.get('tool', []):
            logging.info(
                'Found pyproject.toml with poetry section, '
                'assuming poetry project.')
            apt.install(['python3-venv', 'python3-pip'])
            session.check_call(['pip3', 'install', 'poetry'], user='root')
            session.check_call(['poetry', 'build', '-f', 'sdist'])
            return

    if os.path.exists('setup.py'):
        logging.info('Found setup.py, assuming python project.')
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
            run_with_build_fixer(session, ['./setup.py', 'sdist'])
        else:
            # Just assume it's Python 3
            apt.install(['python3'])
            run_with_build_fixer(session, ['python3', './setup.py', 'sdist'])
        return

    if os.path.exists('setup.cfg'):
        logging.info('Found setup.cfg, assuming python project.')
        apt.install(['python3-pep517', 'python3-pip'])
        session.check_call(['python3', '-m', 'pep517.build', '-s', '.'])
        return

    if os.path.exists('dist.ini') and not os.path.exists('Makefile.PL'):
        apt.install(['libdist-inkt-perl'])
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
                    logging.info(
                        'Found Dist::Inkt section in dist.ini, '
                        'assuming distinkt.')
                    # TODO(jelmer): install via apt if possible
                    session.check_call(
                        ['cpan', 'install', value.decode().strip("'")],
                        user='root')
                    run_with_build_fixer(session, ['distinkt-dist'])
                    return
        # Default to invoking Dist::Zilla
        logging.info('Found dist.ini, assuming dist-zilla.')
        apt.install(['libdist-zilla-perl'])
        run_with_build_fixer(session, ['dzil', 'build', '--in', '..'])
        return

    if os.path.exists('package.json'):
        apt.install(['npm'])
        run_with_build_fixer(session, ['npm', 'pack'])
        return

    gemfiles = [name for name in os.listdir('.') if name.endswith('.gem')]
    if gemfiles:
        apt.install(['gem2deb'])
        if len(gemfiles) > 1:
            logging.warning('More than one gemfile. Trying the first?')
        run_with_build_fixer(session, ['gem2tgz', gemfiles[0]])
        return

    if os.path.exists('waf'):
        apt.install(['python3'])
        run_with_build_fixer(session, ['./waf', 'dist'])
        return

    if os.path.exists('Makefile.PL') and not os.path.exists('Makefile'):
        apt.install(['perl'])
        run_with_build_fixer(session, ['perl', 'Makefile.PL'])

    if not os.path.exists('Makefile') and not os.path.exists('configure'):
        if os.path.exists('autogen.sh'):
            if shebang_binary('autogen.sh') is None:
                run_with_build_fixer(session, ['/bin/sh', './autogen.sh'])
            try:
                run_with_build_fixer(session, ['./autogen.sh'])
            except UnidentifiedError as e:
                if ("Gnulib not yet bootstrapped; "
                        "run ./bootstrap instead.\n" in e.lines):
                    run_with_build_fixer(session, ["./bootstrap"])
                    run_with_build_fixer(session, ['./autogen.sh'])
                else:
                    raise

        elif os.path.exists('configure.ac') or os.path.exists('configure.in'):
            apt.install([
                'autoconf', 'automake', 'gettext', 'libtool', 'gnu-standards'])
            run_with_build_fixer(session, ['autoreconf', '-i'])

    if not os.path.exists('Makefile') and os.path.exists('configure'):
        session.check_call(['./configure'])

    if os.path.exists('Makefile'):
        apt.install(['make'])
        try:
            run_with_build_fixer(session, ['make', 'dist'])
        except UnidentifiedError as e:
            if "make: *** No rule to make target 'dist'.  Stop.\n" in e.lines:
                pass
            elif ("make[1]: *** No rule to make target 'dist'. Stop.\n"
                    in e.lines):
                pass
            elif ("Reconfigure the source tree "
                    "(via './config' or 'perl Configure'), please.\n"
                  ) in e.lines:
                run_with_build_fixer(session, ['./config'])
                run_with_build_fixer(session, ['make', 'dist'])
            elif (
                    "Please try running 'make manifest' and then run "
                    "'make dist' again.\n" in e.lines):
                run_with_build_fixer(session, ['make', 'manifest'])
                run_with_build_fixer(session, ['make', 'dist'])
            elif "Please run ./configure first\n" in e.lines:
                run_with_build_fixer(session, ['./configure'])
                run_with_build_fixer(session, ['make', 'dist'])
            elif any([re.match(
                    r'Makefile:[0-9]+: \*\*\* Missing \'Make.inc\' '
                    r'Run \'./configure \[options\]\' and retry.  Stop.\n',
                    line) for line in e.lines]):
                run_with_build_fixer(session, ['./configure'])
                run_with_build_fixer(session, ['make', 'dist'])
            elif any([re.match(
                      r'Problem opening MANIFEST: No such file or directory '
                      r'at .* line [0-9]+\.', line) for line in e.lines]):
                run_with_build_fixer(session, ['make', 'manifest'])
                run_with_build_fixer(session, ['make', 'dist'])
            else:
                raise
        else:
            return

    raise NoBuildToolsFound()


def export_vcs_tree(tree, directory):
    try:
        export(tree, directory, 'dir', None)
    except OSError as e:
        if e.errno == errno.ENOSPC:
            raise DetailedFailure(
                1, ['export'], NoSpaceOnDevice())
        raise


def dupe_vcs_tree(tree, directory):
    with tree.lock_read():
        if isinstance(tree, WorkingTree):
            tree = tree.basis_tree()
    try:
        result = tree._repository.controldir.sprout(
            directory, create_tree_if_local=True,
            revision_id=tree.get_revision_id())
    except OSError as e:
        if e.errno == errno.ENOSPC:
            raise DetailedFailure(
                1, ['sprout'], NoSpaceOnDevice())
        raise
    if not result.has_workingtree():
        raise AssertionError
    # Copy parent location - some scripts need this
    if isinstance(tree, WorkingTree):
        parent = tree.branch.get_parent()
    else:
        try:
            parent = tree._repository.controldir.open_branch().get_parent()
        except NotBranchError:
            parent = None
    if parent:
        result.open_branch().set_parent(parent)


class DistCatcher(object):

    def __init__(self, directory):
        self.export_directory = directory
        self.files = []
        self.existing_files = None

    def __enter__(self):
        self.existing_files = os.listdir(self.export_directory)
        return self

    def find_files(self):
        new_files = os.listdir(self.export_directory)
        diff_files = set(new_files) - set(self.existing_files)
        diff = set([n for n in diff_files if get_filetype(n) is not None])
        if len(diff) == 1:
            fn = diff.pop()
            logging.info('Found tarball %s in package directory.', fn)
            self.files.append(os.path.join(self.export_directory, fn))
            return fn
        if 'dist' in diff_files:
            for entry in os.scandir(
                    os.path.join(self.export_directory, 'dist')):
                if get_filetype(entry.name) is not None:
                    logging.info(
                        'Found tarball %s in dist directory.', entry.name)
                    self.files.append(entry.path)
                    return entry.name
            logging.info('No tarballs found in dist directory.')

        parent_directory = os.path.dirname(self.export_directory)
        diff = set(os.listdir(parent_directory)) - set([subdir])
        if len(diff) == 1:
            fn = diff.pop()
            logging.info('Found tarball %s in parent directory.', fn)
            self.files.append(os.path.join(parent_directory, fn))
            return fn

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.find_files()
        return False


def create_dist_schroot(
        tree: Tree, target_dir: str,
        chroot: str, packaging_tree: Optional[Tree] = None,
        include_controldir: bool = True,
        subdir: Optional[str] = None) -> str:
    if subdir is None:
        subdir = 'package'
    with SchrootSession(chroot) as session:
        if packaging_tree is not None:
            satisfy_build_deps(session, packaging_tree)
        build_dir = os.path.join(session.location, 'build')

        try:
            directory = tempfile.mkdtemp(dir=build_dir)
        except OSError as e:
            if e.errno == errno.ENOSPC:
                raise DetailedFailure(
                    1, ['mkdtemp'], NoSpaceOnDevice())
        reldir = '/' + os.path.relpath(directory, session.location)

        export_directory = os.path.join(directory, subdir)
        if not include_controldir:
            export_vcs_tree(tree, export_directory)
        else:
            dupe_vcs_tree(tree, export_directory)

        with DistCatcher(export_directory) as dc:
            oldcwd = os.getcwd()
            os.chdir(export_directory)
            try:
                session.chdir(os.path.join(reldir, subdir))
                run_dist(session)
            finally:
                os.chdir(oldcwd)

        for path in dc.files:
            shutil.copy(path, target_dir)
            return os.path.join(target_dir, os.path.basename(path))

        logging.info('No tarball created :(')
        raise DistNoTarball()


if __name__ == '__main__':
    import argparse
    import breezy.bzr  # noqa: F401
    import breezy.git  # noqa: F401

    parser = argparse.ArgumentParser()
    parser.add_argument(
        '--chroot', default='unstable-amd64-sbuild', type=str,
        help='Name of chroot to use')
    parser.add_argument(
        'directory', default='.', type=str, nargs='?',
        help='Directory with upstream source.')
    parser.add_argument(
        '--packaging-directory', type=str,
        help='Path to packaging directory.')
    parser.add_argument(
        '--target-directory', type=str, default='..',
        help='Target directory')
    args = parser.parse_args()

    logging.basicConfig(level=logging.INFO)

    tree = WorkingTree.open(args.directory)
    if args.packaging_directory:
        packaging_tree = WorkingTree.open(args.packaging_directory)
        with packaging_tree.lock_read():
            source = Deb822(packaging_tree.get_file('debian/control'))
        package = source['Source']
        subdir = package
    else:
        packaging_tree = None
        subdir = None

    try:
        ret = create_dist_schroot(
            tree, subdir=subdir,
            target_dir=os.path.abspath(args.target_directory),
            packaging_tree=packaging_tree,
            chroot=args.chroot)
    except NoBuildToolsFound:
        logging.info('No build tools found, falling back to simple export.')
        export(tree, 'dist.tar.gz', 'tgz', None)
    else:
        print('Created %s' % ret)
    sys.exit(0)

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

import logging
from typing import List, Tuple, Callable, Type

from buildlog_consultant.sbuild import (
    find_build_failure_description,
    Problem,
    MissingPerlModule,
    MissingCommand,
    )

from . import DetailedFailure
from .apt import UnidentifiedError, AptManager
from .debian.fix_build import (
    DependencyContext,
    resolve_error,
    APT_FIXERS,
    )
from .session import Session, run_with_tee


class SchrootDependencyContext(DependencyContext):

    def __init__(self, session):
        self.session = session
        self.apt = AptManager(session)

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

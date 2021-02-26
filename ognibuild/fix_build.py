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
from typing import List, Tuple, Callable, Type, Optional

from buildlog_consultant.common import (
    find_build_failure_description,
    Problem,
    MissingPerlModule,
    MissingPythonDistribution,
    MissingCommand,
)
from breezy.mutabletree import MutableTree

from . import DetailedFailure, UnidentifiedError
from .debian.apt import AptManager
from .session import Session, run_with_tee


class BuildFixer(object):
    """Build fixer."""

    def can_fix(self, problem):
        raise NotImplementedError(self.can_fix)

    def _fix(self, problem, context):
        raise NotImplementedError(self._fix)

    def fix(self, problem, context):
        if not self.can_fix(problem):
            return None
        return self._fix(problem, context)


class DependencyContext(object):
    def __init__(
        self,
        tree: MutableTree,
        apt: AptManager,
        subpath: str = "",
        committer: Optional[str] = None,
        update_changelog: bool = True,
    ):
        self.tree = tree
        self.apt = apt
        self.subpath = subpath
        self.committer = committer
        self.update_changelog = update_changelog

    def add_dependency(
        self, package: str, minimum_version: Optional['Version'] = None
    ) -> bool:
        raise NotImplementedError(self.add_dependency)


class SchrootDependencyContext(DependencyContext):
    def __init__(self, session):
        self.session = session
        self.apt = AptManager(session)

    def add_dependency(self, package, minimum_version=None):
        # TODO(jelmer): Handle minimum_version
        self.apt.install([package])
        return True


def generic_install_fixers(session):
    from .buildlog import UpstreamRequirementFixer
    from .resolver import CPANResolver, PypiResolver, NpmResolver
    return [
        UpstreamRequirementFixer(CPANResolver(session)),
        UpstreamRequirementFixer(PypiResolver(session)),
        UpstreamRequirementFixer(NpmResolver(session)),
        ]


def run_with_build_fixer(
        session: Session, args: List[str],
        fixers: Optional[List[BuildFixer]] = None):
    if fixers is None:
        from .debian.fix_build import apt_fixers
        from .resolver.apt import AptResolver
        fixers = generic_install_fixers(session) + apt_fixers(AptResolver.from_session(session))
    logging.info("Running %r", args)
    fixed_errors = []
    while True:
        retcode, lines = run_with_tee(session, args)
        if retcode == 0:
            return
        match, error = find_build_failure_description(lines)
        if error is None:
            logging.warning("Build failed with unidentified error. Giving up.")
            if match is not None:
                raise UnidentifiedError(
                    retcode, args, lines, secondary=(match.lineno, match.line))
            raise UnidentifiedError(retcode, args, lines)

        logging.info("Identified error: %r", error)
        if error in fixed_errors:
            logging.warning(
                "Failed to resolve error %r, it persisted. Giving up.", error
            )
            raise DetailedFailure(retcode, args, error)
        if not resolve_error(
            error,
            SchrootDependencyContext(session),
            fixers=fixers,
        ):
            logging.warning("Failed to find resolution for error %r. Giving up.", error)
            raise DetailedFailure(retcode, args, error)
        fixed_errors.append(error)


def resolve_error(error, context, fixers):
    relevant_fixers = []
    for fixer in fixers:
        if fixer.can_fix(error):
            relevant_fixers.append(fixer)
    if not relevant_fixers:
        logging.warning("No fixer found for %r", error)
        return False
    for fixer in relevant_fixers:
        logging.info("Attempting to use fixer %r to address %r", fixer, error)
        made_changes = fixer.fix(error, context)
        if made_changes:
            return True
    return False

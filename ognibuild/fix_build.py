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
from typing import List, Optional

from buildlog_consultant.common import (
    find_build_failure_description,
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

    def add_dependency(self, package) -> bool:
        raise NotImplementedError(self.add_dependency)


def run_with_build_fixers(session: Session, args: List[str], fixers: List[BuildFixer]):
    logging.info("Running %r", args)
    fixed_errors = []
    while True:
        retcode, lines = run_with_tee(session, args)
        if retcode == 0:
            return
        match, error = find_build_failure_description(lines)
        if error is None:
            if match:
                logging.warning("Build failed with unidentified error:")
                logging.warning("%s", match.line.rstrip("\n"))
            else:
                logging.warning("Build failed and unable to find cause. Giving up.")
            raise UnidentifiedError(retcode, args, lines, secondary=match)

        logging.info("Identified error: %r", error)
        if error in fixed_errors:
            logging.warning(
                "Failed to resolve error %r, it persisted. Giving up.", error
            )
            raise DetailedFailure(retcode, args, error)
        if not resolve_error(
            error,
            None,
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
        logging.info("Attempting to use fixer %s to address %r", fixer, error)
        made_changes = fixer.fix(error, context)
        if made_changes:
            return True
    return False

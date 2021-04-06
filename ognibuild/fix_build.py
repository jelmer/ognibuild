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

from functools import partial
import logging
from typing import List, Tuple, Callable, Any, Optional

from buildlog_consultant import Problem
from buildlog_consultant.common import (
    find_build_failure_description,
    MissingCommand,
)

from . import DetailedFailure, UnidentifiedError
from .session import Session, run_with_tee


class BuildFixer(object):
    """Build fixer."""

    def can_fix(self, problem: Problem):
        raise NotImplementedError(self.can_fix)

    def _fix(self, problem: Problem, phase: Tuple[str, ...]):
        raise NotImplementedError(self._fix)

    def fix(self, problem: Problem, phase: Tuple[str, ...]):
        if not self.can_fix(problem):
            return None
        return self._fix(problem, phase)


def run_detecting_problems(session: Session, args: List[str], **kwargs):
    try:
        retcode, contents = run_with_tee(session, args, **kwargs)
    except FileNotFoundError:
        error = MissingCommand(args[0])
        retcode = 1
    else:
        if retcode == 0:
            return contents
        lines = "".join(contents).splitlines(False)
        match, error = find_build_failure_description(lines)
        if error is None:
            if match:
                logging.warning("Build failed with unidentified error:")
                logging.warning("%s", match.line.rstrip("\n"))
            else:
                logging.warning("Build failed and unable to find cause. Giving up.")
            raise UnidentifiedError(retcode, args, lines, secondary=match)
    raise DetailedFailure(retcode, args, error)


def iterate_with_build_fixers(fixers: List[BuildFixer], cb: Callable[[], Any]):
    """Call cb() until there are no more DetailedFailures we can fix.

    Args:
      fixers: List of fixers to use to resolve issues
    """
    fixed_errors = []
    while True:
        to_resolve = []
        try:
            return cb()
        except DetailedFailure as e:
            to_resolve.append(e)
        while to_resolve:
            f = to_resolve.pop(-1)
            logging.info("Identified error: %r", f.error)
            if f.error in fixed_errors:
                logging.warning(
                    "Failed to resolve error %r, it persisted. Giving up.", f.error
                )
                raise f
            try:
                resolved = resolve_error(f.error, None, fixers=fixers)
            except DetailedFailure as n:
                logging.info("New error %r while resolving %r", n, f)
                if n in to_resolve:
                    raise
                to_resolve.append(f)
                to_resolve.append(n)
            else:
                if not resolved:
                    logging.warning(
                        "Failed to find resolution for error %r. Giving up.", f.error
                    )
                    raise f
                fixed_errors.append(f.error)


def run_with_build_fixers(
    session: Session, args: List[str], fixers: Optional[List[BuildFixer]], **kwargs
):
    if fixers is None:
        fixers = []
    return iterate_with_build_fixers(
        fixers, partial(run_detecting_problems, session, args, **kwargs)
    )


def resolve_error(error, phase, fixers):
    relevant_fixers = []
    for fixer in fixers:
        if fixer.can_fix(error):
            relevant_fixers.append(fixer)
    if not relevant_fixers:
        logging.warning("No fixer found for %r", error)
        return False
    for fixer in relevant_fixers:
        logging.info("Attempting to use fixer %s to address %r", fixer, error)
        made_changes = fixer.fix(error, phase)
        if made_changes:
            return True
    return False

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

__all__ = [
    "resolve_error",
    "iterate_with_build_fixers",
    "run_with_build_fixers",
    "run_detecting_problems",
]

import logging
from functools import partial
from typing import Callable, Optional

from buildlog_consultant import Problem
from buildlog_consultant.common import (
    MissingCommand,
    find_build_failure_description,
)

from . import DetailedFailure, UnidentifiedError
from ._ognibuild_rs import iterate_with_build_fixers, resolve_error
from .session import Session, run_with_tee


class FixerLimitReached(Exception):
    """The maximum number of fixes has been reached."""


class BuildFixer:
    """Build fixer."""

    def can_fix(self, problem: Problem):
        raise NotImplementedError(self.can_fix)

    def _fix(self, problem: Problem, phase: tuple[str, ...]):
        raise NotImplementedError(self._fix)

    def fix(self, problem: Problem, phase: tuple[str, ...]):
        if not self.can_fix(problem):
            return None
        return self._fix(problem, phase)


def run_detecting_problems(
    session: Session,
    args: list[str],
    check_success: Optional[Callable[[int, list[str]], bool]] = None,
    quiet=False,
    **kwargs,
) -> list[str]:
    error: Optional[Problem]
    if not quiet:
        logging.info("Running %r", args)
    if check_success is None:

        def check_success(retcode, contents):
            return retcode == 0

    try:
        retcode, contents = run_with_tee(session, args, **kwargs)
    except FileNotFoundError:
        error = MissingCommand(args[0])
        retcode = 1
    else:
        if check_success(retcode, contents):
            return contents
        lines = "".join(contents).splitlines(False)
        match, error = find_build_failure_description(lines)
        if error is None:
            if match:
                logging.warning("Build failed with unidentified error:")
                logging.warning("%s", match.line.rstrip("\n"))
            else:
                logging.warning(
                    "Build failed and unable to find cause. Giving up."
                )
            raise UnidentifiedError(retcode, args, lines, secondary=match)
    raise DetailedFailure(retcode, args, error)


def run_with_build_fixers(
    fixers: Optional[list[BuildFixer]],
    session: Session,
    args: list[str],
    quiet: bool = False,
    **kwargs,
) -> list[str]:
    if fixers is None:
        fixers = []
    return iterate_with_build_fixers(
        fixers,
        ["build"],
        partial(run_detecting_problems, session, args, quiet=quiet, **kwargs),
    )

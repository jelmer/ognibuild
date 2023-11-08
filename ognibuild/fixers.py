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

import re
import subprocess

from buildlog_consultant import Problem
from buildlog_consultant.common import (
    MinimumAutoconfTooOld,
    MissingAutoconfMacro,
    MissingGitIdentity,
    MissingGnulibDirectory,
    MissingGoSumEntry,
    MissingSecretGpgKey,
)

from ognibuild.requirements import AutoconfMacroRequirement
from ognibuild.resolver import UnsatisfiedRequirements

from .fix_build import BuildFixer


class GnulibDirectoryFixer(BuildFixer):
    def __init__(self, session) -> None:
        self.session = session

    def can_fix(self, problem: Problem):
        return isinstance(problem, MissingGnulibDirectory)

    def _fix(self, problem: Problem, phase: tuple[str, ...]):
        self.session.check_call(["./gnulib.sh"])
        return True


class GitIdentityFixer(BuildFixer):
    def __init__(self, session) -> None:
        self.session = session

    def can_fix(self, problem: Problem):
        return isinstance(problem, MissingGitIdentity)

    def _fix(self, problem: Problem, phase: tuple[str, ...]):
        for name in ["user.email", "user.name"]:
            value = (
                subprocess.check_output(["git", "config", "--global", name])
                .decode()
                .strip()
            )
            self.session.check_call(["git", "config", "--global", name, value])
        return True


class SecretGpgKeyFixer(BuildFixer):
    def __init__(self, session) -> None:
        self.session = session

    def can_fix(self, problem: Problem):
        return isinstance(problem, MissingSecretGpgKey)

    def _fix(self, problem: Problem, phase: tuple[str, ...]):
        SCRIPT = b"""\
Key-Type: 1
Key-Length: 4096
Subkey-Type: 1
Subkey-Length: 4096
Name-Real: Dummy Key for ognibuild
Name-Email: dummy@example.com
Expire-Date: 0
Passphrase: ""
"""
        p = self.session.Popen(
            ["gpg", "--gen-key", "--batch", "/dev/stdin"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
        )
        p.communicate(SCRIPT)
        return p.returncode == 0


class MinimumAutoconfFixer(BuildFixer):
    def __init__(self, session) -> None:
        self.session = session

    def can_fix(self, problem: Problem):
        return isinstance(problem, MinimumAutoconfTooOld)

    def _fix(self, error, phase):
        for name in ["configure.ac", "configure.in"]:
            try:
                with open(self.session.external_path(name), "rb") as f:
                    lines = list(f.readlines())
            except FileNotFoundError:
                continue
            pattern = re.compile(rb"AC_PREREQ\((.*)\)")
            for i, line in enumerate(lines):
                m = pattern.fullmatch(line)
                if not m:
                    continue
                lines[i] = f"AC_PREREQ([{error.minimum_version}])".encode(
                    "ascii"
                )
            else:
                lines.insert(
                    0, f"AC_PREREQ([{error.minimum_version}])".encode("ascii")
                )
            with open(self.session.external_path(name), "wb") as f:
                f.writelines(lines)
            return True
        return False


class MissingGoSumEntryFixer(BuildFixer):
    def __init__(self, session) -> None:
        self.session = session

    def __repr__(self) -> str:
        return "%s()" % (type(self).__name__)

    def __str__(self) -> str:
        return "missing go.sum entry fixer"

    def can_fix(self, error):
        return isinstance(error, MissingGoSumEntry)

    def _fix(self, error, phase):
        from .fix_build import run_detecting_problems

        run_detecting_problems(
            self.session, ["go", "mod", "download", error.package]
        )
        return True


class UnexpandedAutoconfMacroFixer(BuildFixer):
    def __init__(self, session, resolver) -> None:
        self.session = session
        self.resolver = resolver

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.resolver!r})"

    def __str__(self) -> str:
        return "unexpanded m4 macro fixer (%s)" % self.resolver

    def can_fix(self, error):
        return isinstance(error, MissingAutoconfMacro)

    def _fix(self, error, phase):
        try:
            self.resolver.install([AutoconfMacroRequirement(error.macro)])
        except UnsatisfiedRequirements:
            return False
        from .fix_build import run_detecting_problems

        run_detecting_problems(self.session, ["autoconf", "-f"])

        return True

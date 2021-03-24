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

import subprocess
from typing import Tuple

from buildlog_consultant import Problem
from buildlog_consultant.fix_build import BuildFixer
from buildlog_consultant.common import (
    MissingGitIdentity,
    )


class GitIdentityFixer(BuildFixer):

    def __init__(self, session):
        self.session = session

    def can_fix(self, problem: Problem):
        return isinstance(problem, MissingGitIdentity)

    def _fix(self, problem: Problem, phase: Tuple[str, ...]):
        for name in ['user.email', 'user.name']:
            value = subprocess.check_output(
                ['git', 'config', '--global', name]).decode().strip()
            self.session.check_call(
                ['git', 'config', '--global', 'user.email', value])
        return True

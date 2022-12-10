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

"""Convert problems found in the buildlog to upstream requirements.
"""

import logging
from typing import Optional, List, cast

from buildlog_consultant.common import (
    Problem,
    MissingPerlFile,
    MissingSetupPyCommand,
    MissingCMakeComponents,
    MissingXfceDependency,
    MissingHaskellDependencies,
    MissingMavenArtifacts,
    MissingGnomeCommonDependency,
    MissingPerlPredeclared,
    MissingLatexFile,
    MissingCargoCrate,
)

from . import OneOfRequirement
from .fix_build import BuildFixer
from .requirements import (
    Requirement,
    BinaryRequirement,
    HaskellPackageRequirement,
    MavenArtifactRequirement,
    BoostComponentRequirement,
    KF5ComponentRequirement,
    PerlFileRequirement,
    PythonPackageRequirement,
    PerlPreDeclaredRequirement,
    LatexPackageRequirement,
    CargoCrateRequirement,
)
from .resolver import UnsatisfiedRequirements
from .buildlog_converters import PROBLEM_CONVERTERS  # type: ignore


def problem_to_upstream_requirement(
        problem: Problem) -> Optional[Requirement]:  # noqa: C901
    for entry in PROBLEM_CONVERTERS:
        kind, fn = entry[:2]
        if kind == problem.kind:
            return fn(problem)
    if isinstance(problem, MissingCMakeComponents):
        if problem.name.lower() == 'boost':
            return OneOfRequirement(
                [BoostComponentRequirement(name)
                 for name in problem.components])
        elif problem.name.lower() == 'kf5':
            return OneOfRequirement(
                [KF5ComponentRequirement(name) for name in problem.components])
        return None
    elif isinstance(problem, MissingLatexFile):
        if problem.filename.endswith('.sty'):
            return LatexPackageRequirement(problem.filename[:-4])
        return None
    elif isinstance(problem, MissingHaskellDependencies):
        return OneOfRequirement(
            [HaskellPackageRequirement.from_string(dep)
             for dep in problem.deps])
    elif isinstance(problem, MissingMavenArtifacts):
        return OneOfRequirement([
            MavenArtifactRequirement.from_str(artifact)
            for artifact in problem.artifacts
        ])
    elif isinstance(problem, MissingPerlPredeclared):
        ret = PerlPreDeclaredRequirement(problem.name)
        try:
            return ret.lookup_module()
        except KeyError:
            return ret
    elif isinstance(problem, MissingCargoCrate):
        # TODO(jelmer): handle problem.requirements
        return CargoCrateRequirement(problem.crate)
    elif isinstance(problem, MissingSetupPyCommand):
        if problem.command == "test":
            return PythonPackageRequirement("setuptools")
        return None
    elif isinstance(problem, MissingGnomeCommonDependency):
        if problem.package == "glib-gettext":
            return BinaryRequirement("glib-gettextize")
        else:
            logging.warning(
                "No known command for gnome-common dependency %s",
                problem.package
            )
            return None
    elif isinstance(problem, MissingXfceDependency):
        if problem.package == "gtk-doc":
            return BinaryRequirement("gtkdocize")
        else:
            logging.warning(
                "No known command for xfce dependency %s", problem.package)
            return None
    elif isinstance(problem, MissingPerlFile):
        return PerlFileRequirement(filename=problem.filename)
    elif problem.kind == 'unsatisfied-apt-dependencies':
        from buildlog_consultant.apt import UnsatisfiedAptDependencies
        from .resolver.apt import AptRequirement
        return AptRequirement(
            cast(UnsatisfiedAptDependencies, problem).relations)
    else:
        logging.warning(
            'Unable to determine how to deal with %r',
            problem)
        return None


class InstallFixer(BuildFixer):
    def __init__(self, resolver):
        self.resolver = resolver

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.resolver)

    def __str__(self):
        return "upstream requirement fixer(%s)" % self.resolver

    def can_fix(self, error):
        req = problem_to_upstream_requirement(error)
        return req is not None

    def fix(self, error, phase):
        req = problem_to_upstream_requirement(error)
        if req is None:
            return False

        reqs: List[Requirement]
        if not isinstance(req, list):
            reqs = [req]
        else:
            reqs = req

        try:
            self.resolver.install(reqs)
        except UnsatisfiedRequirements:
            return False
        return True


class ExplainInstall(Exception):
    def __init__(self, commands):
        self.commands = commands


class ExplainInstallFixer(BuildFixer):
    def __init__(self, resolver):
        self.resolver = resolver

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.resolver)

    def __str__(self):
        return "upstream requirement install explainer(%s)" % self.resolver

    def can_fix(self, error):
        req = problem_to_upstream_requirement(error)
        return req is not None

    def fix(self, error, phase):
        req = problem_to_upstream_requirement(error)
        if req is None:
            return False

        if not isinstance(req, list):
            reqs = [req]
        else:
            reqs = req

        explanations = list(self.resolver.explain(reqs))
        if not explanations:
            return False
        raise ExplainInstall(explanations)


def install_missing_reqs(session, resolver, reqs, explain=False):
    if not reqs:
        return
    missing = []
    for req in reqs:
        try:
            if not req.met(session):
                missing.append(req)
        except NotImplementedError:
            missing.append(req)
    if missing:
        if explain:
            commands = resolver.explain(missing)
            if not commands:
                raise UnsatisfiedRequirements(missing)
            raise ExplainInstall(commands)
        else:
            resolver.install(missing)

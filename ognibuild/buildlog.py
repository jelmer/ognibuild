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

from buildlog_consultant.common import (
    MissingPythonModule,
    MissingPythonDistribution,
    MissingCHeader,
    MissingPkgConfig,
    MissingCommand,
    MissingFile,
    MissingJavaScriptRuntime,
    MissingSprocketsFile,
    MissingGoPackage,
    MissingPerlFile,
    MissingPerlModule,
    MissingXmlEntity,
    MissingJDKFile,
    MissingNodeModule,
    MissingPhpClass,
    MissingRubyGem,
    MissingLibrary,
    MissingJavaClass,
    MissingCSharpCompiler,
    MissingRPackage,
    MissingRubyFile,
    MissingAutoconfMacro,
    MissingValaPackage,
    MissingXfceDependency,
    MissingHaskellDependencies,
    DhAddonLoadFailure,
    MissingMavenArtifacts,
    GnomeCommonMissing,
    MissingGnomeCommonDependency,
)

from .fix_build import BuildFixer
from .requirements import (
    BinaryRequirement,
    PathRequirement,
    PkgConfigRequirement,
    CHeaderRequirement,
    JavaScriptRuntimeRequirement,
    ValaPackageRequirement,
    RubyGemRequirement,
    GoPackageRequirement,
    DhAddonRequirement,
    PhpClassRequirement,
    RPackageRequirement,
    NodePackageRequirement,
    LibraryRequirement,
    RubyFileRequirement,
    XmlEntityRequirement,
    SprocketsFileRequirement,
    JavaClassRequirement,
    HaskellPackageRequirement,
    MavenArtifactRequirement,
    GnomeCommonRequirement,
    JDKFileRequirement,
    PerlModuleRequirement,
    PerlFileRequirement,
    AutoconfMacroRequirement,
    PythonModuleRequirement,
    PythonPackageRequirement,
)
from .resolver import UnsatisfiedRequirements


def problem_to_upstream_requirement(problem):  # noqa: C901
    if isinstance(problem, MissingFile):
        return PathRequirement(problem.path)
    elif isinstance(problem, MissingCommand):
        return BinaryRequirement(problem.command)
    elif isinstance(problem, MissingPkgConfig):
        return PkgConfigRequirement(problem.module, problem.minimum_version)
    elif isinstance(problem, MissingCHeader):
        return CHeaderRequirement(problem.header)
    elif isinstance(problem, MissingJavaScriptRuntime):
        return JavaScriptRuntimeRequirement()
    elif isinstance(problem, MissingRubyGem):
        return RubyGemRequirement(problem.gem, problem.version)
    elif isinstance(problem, MissingValaPackage):
        return ValaPackageRequirement(problem.package)
    elif isinstance(problem, MissingGoPackage):
        return GoPackageRequirement(problem.package)
    elif isinstance(problem, DhAddonLoadFailure):
        return DhAddonRequirement(problem.path)
    elif isinstance(problem, MissingPhpClass):
        return PhpClassRequirement(problem.php_class)
    elif isinstance(problem, MissingRPackage):
        return RPackageRequirement(problem.package, problem.minimum_version)
    elif isinstance(problem, MissingNodeModule):
        return NodePackageRequirement(problem.module)
    elif isinstance(problem, MissingLibrary):
        return LibraryRequirement(problem.library)
    elif isinstance(problem, MissingRubyFile):
        return RubyFileRequirement(problem.filename)
    elif isinstance(problem, MissingXmlEntity):
        return XmlEntityRequirement(problem.url)
    elif isinstance(problem, MissingSprocketsFile):
        return SprocketsFileRequirement(problem.content_type, problem.name)
    elif isinstance(problem, MissingJavaClass):
        return JavaClassRequirement(problem.classname)
    elif isinstance(problem, MissingHaskellDependencies):
        return [HaskellPackageRequirement.from_string(dep) for dep in problem.deps]
    elif isinstance(problem, MissingMavenArtifacts):
        return [MavenArtifactRequirement(artifact) for artifact in problem.artifacts]
    elif isinstance(problem, MissingCSharpCompiler):
        return BinaryRequirement("msc")
    elif isinstance(problem, GnomeCommonMissing):
        return GnomeCommonRequirement()
    elif isinstance(problem, MissingJDKFile):
        return JDKFileRequirement(problem.jdk_path, problem.filename)
    elif isinstance(problem, MissingGnomeCommonDependency):
        if problem.package == "glib-gettext":
            return BinaryRequirement("glib-gettextize")
        else:
            logging.warning(
                "No known command for gnome-common dependency %s", problem.package
            )
            return None
    elif isinstance(problem, MissingXfceDependency):
        if problem.package == "gtk-doc":
            return BinaryRequirement("gtkdocize")
        else:
            logging.warning("No known command for xfce dependency %s", problem.package)
            return None
    elif isinstance(problem, MissingPerlModule):
        return PerlModuleRequirement(
            module=problem.module, filename=problem.filename, inc=problem.inc
        )
    elif isinstance(problem, MissingPerlFile):
        return PerlFileRequirement(filename=problem.filename)
    elif isinstance(problem, MissingAutoconfMacro):
        return AutoconfMacroRequirement(problem.macro)
    elif isinstance(problem, MissingPythonModule):
        return PythonModuleRequirement(
            problem.module,
            python_version=problem.python_version,
            minimum_version=problem.minimum_version,
        )
    elif isinstance(problem, MissingPythonDistribution):
        return PythonPackageRequirement(
            problem.module,
            python_version=problem.python_version,
            minimum_version=problem.minimum_version,
        )
    else:
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

    def fix(self, error, context):
        reqs = problem_to_upstream_requirement(error)
        if reqs is None:
            return False

        if not isinstance(reqs, list):
            reqs = [reqs]

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

    def fix(self, error, context):
        reqs = problem_to_upstream_requirement(error)
        if reqs is None:
            return False

        if not isinstance(reqs, list):
            reqs = [reqs]

        explanations = list(self.resolver.explain(reqs))
        if not explanations:
            return False
        raise ExplainInstall(explanations)

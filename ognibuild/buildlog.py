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
from typing import Optional

from buildlog_consultant.common import (
    Problem,
    MissingSprocketsFile,
    MissingGoPackage,
    MissingPerlFile,
    MissingPerlModule,
    MissingXmlEntity,
    MissingJDKFile,
    MissingJDK,
    MissingJRE,
    MissingPhpClass,
    MissingLibrary,
    MissingSetupPyCommand,
    MissingJavaClass,
    MissingCSharpCompiler,
    MissingRustCompiler,
    MissingRPackage,
    MissingRubyFile,
    MissingAutoconfMacro,
    MissingValaPackage,
    MissingCMakeComponents,
    MissingXfceDependency,
    MissingHaskellDependencies,
    MissingVagueDependency,
    DhAddonLoadFailure,
    MissingMavenArtifacts,
    GnomeCommonMissing,
    MissingGnomeCommonDependency,
    UnknownCertificateAuthority,
    MissingLibtool,
    MissingQt,
    MissingX11,
    MissingPerlPredeclared,
    MissingLatexFile,
    MissingCargoCrate,
    MissingStaticLibrary,
    MissingGnulibDirectory,
    MissingLuaModule,
    MissingPHPExtension,
    VcsControlDirectoryNeeded,
)

from . import OneOfRequirement
from .fix_build import BuildFixer
from .requirements import (
    Requirement,
    BinaryRequirement,
    PathRequirement,
    PkgConfigRequirement,
    CHeaderRequirement,
    JavaScriptRuntimeRequirement,
    ValaPackageRequirement,
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
    CMakefileRequirement,
    HaskellPackageRequirement,
    MavenArtifactRequirement,
    BoostComponentRequirement,
    KF5ComponentRequirement,
    GnomeCommonRequirement,
    JDKFileRequirement,
    JDKRequirement,
    JRERequirement,
    PerlModuleRequirement,
    PerlFileRequirement,
    AutoconfMacroRequirement,
    PythonModuleRequirement,
    PythonPackageRequirement,
    CertificateAuthorityRequirement,
    NodeModuleRequirement,
    QTRequirement,
    X11Requirement,
    LibtoolRequirement,
    VagueDependencyRequirement,
    IntrospectionTypelibRequirement,
    PerlPreDeclaredRequirement,
    LatexPackageRequirement,
    CargoCrateRequirement,
    StaticLibraryRequirement,
    GnulibDirectoryRequirement,
    LuaModuleRequirement,
    PHPExtensionRequirement,
    VcsControlDirectoryAccessRequirement,
    RubyGemRequirement,
)
from .resolver import UnsatisfiedRequirements


PROBLEM_CONVERTERS = [
    ('missing-file', lambda p: PathRequirement(p.path)),
    ('command-missing', lambda p: BinaryRequirement(p.command)),
    ('missing-cmake-config', lambda p: OneOfRequirement(
        [CMakefileRequirement(filename, p.version) for filename in p.filenames])),
    ('missing-command-or-build-file', lambda p: BinaryRequirement(p.command)),
    ('missing-pkg-config-package', lambda p: PkgConfigRequirement(p.module, p.minimum_version)),
    ('missing-c-header', lambda p: CHeaderRequirement(p.header)),
    ('missing-introspection-typelib', lambda p: IntrospectionTypelibRequirement(p.library)),
    ('missing-python-module', lambda p: PythonModuleRequirement(
        p.module, python_version=p.python_version, minimum_version=p.minimum_version)),
    ('missing-python-distribution', lambda p: PythonPackageRequirement(
        p.distribution, python_version=p.python_version, minimum_version=p.minimum_version)),
    ('javascript-runtime-missing', lambda p: JavaScriptRuntimeRequirement()),
    ('missing-node-module', lambda p: NodeModuleRequirement(p.module)),
    ('missing-node-package', lambda p: NodePackageRequirement(p.package)),
    ('missing-ruby-gem', lambda p: RubyGemRequirement(p.gem, p.version)),
]


def problem_to_upstream_requirement(problem: Problem) -> Optional[Requirement]:  # noqa: C901
    for kind, fn in PROBLEM_CONVERTERS:
        if kind == problem.kind:
            return fn(problem)
    if isinstance(problem, MissingValaPackage):
        return ValaPackageRequirement(problem.package)
    elif isinstance(problem, MissingGoPackage):
        return GoPackageRequirement(problem.package)
    elif isinstance(problem, MissingCMakeComponents):
        if problem.name.lower() == 'boost':
            return OneOfRequirement(
                [BoostComponentRequirement(name) for name in problem.components])
        elif problem.name.lower() == 'kf5':
            return OneOfRequirement(
                [KF5ComponentRequirement(name) for name in problem.components])
        return None
    elif isinstance(problem, DhAddonLoadFailure):
        return DhAddonRequirement(problem.path)
    elif isinstance(problem, MissingPhpClass):
        return PhpClassRequirement(problem.php_class)
    elif isinstance(problem, MissingRPackage):
        return RPackageRequirement(problem.package, problem.minimum_version)
    elif isinstance(problem, MissingStaticLibrary):
        return StaticLibraryRequirement(problem.library, problem.filename)
    elif isinstance(problem, MissingLuaModule):
        return LuaModuleRequirement(problem.module)
    elif isinstance(problem, MissingLatexFile):
        if problem.filename.endswith('.sty'):
            return LatexPackageRequirement(problem.filename[:-4])
        return None
    elif isinstance(problem, MissingVagueDependency):
        return VagueDependencyRequirement(problem.name, minimum_version=problem.minimum_version)
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
        return OneOfRequirement(
            [HaskellPackageRequirement.from_string(dep) for dep in problem.deps])
    elif isinstance(problem, MissingMavenArtifacts):
        return OneOfRequirement([
            MavenArtifactRequirement.from_str(artifact)
            for artifact in problem.artifacts
        ])
    elif isinstance(problem, MissingCSharpCompiler):
        return BinaryRequirement("msc")
    elif isinstance(problem, VcsControlDirectoryNeeded):
        return VcsControlDirectoryAccessRequirement(problem.vcs)
    elif isinstance(problem, MissingRustCompiler):
        return BinaryRequirement("rustc")
    elif isinstance(problem, GnomeCommonMissing):
        return GnomeCommonRequirement()
    elif isinstance(problem, MissingGnulibDirectory):
        return GnulibDirectoryRequirement(problem.directory)
    elif isinstance(problem, MissingJDKFile):
        return JDKFileRequirement(problem.jdk_path, problem.filename)
    elif isinstance(problem, MissingJDK):
        return JDKRequirement()
    elif isinstance(problem, MissingJRE):
        return JRERequirement()
    elif isinstance(problem, MissingQt):
        return QTRequirement()
    elif isinstance(problem, MissingX11):
        return X11Requirement()
    elif isinstance(problem, MissingLibtool):
        return LibtoolRequirement()
    elif isinstance(problem, UnknownCertificateAuthority):
        return CertificateAuthorityRequirement(problem.url)
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
    elif isinstance(problem, MissingPHPExtension):
        return PHPExtensionRequirement(problem.extension)
    elif isinstance(problem, MissingPerlFile):
        return PerlFileRequirement(filename=problem.filename)
    elif isinstance(problem, MissingAutoconfMacro):
        return AutoconfMacroRequirement(problem.macro)
    elif problem.kind == 'unsatisfied-apt-dependencies':
        from .resolver.apt import AptRequirement
        return AptRequirement(problem.relations)
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

    def fix(self, error, phase):
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

    def fix(self, error, phase):
        reqs = problem_to_upstream_requirement(error)
        if reqs is None:
            return False

        if not isinstance(reqs, list):
            reqs = [reqs]

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

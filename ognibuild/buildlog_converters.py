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

# TODO(jelmer): don't ignore all typing here
# type: ignore

"""Convert problems found in the buildlog to upstream requirements."""

from typing import Callable, Optional, Union

from buildlog_consultant.common import Problem

from . import OneOfRequirement
from .requirements import (
    AutoconfMacroRequirement,
    BinaryRequirement,
    CertificateAuthorityRequirement,
    CHeaderRequirement,
    CMakefileRequirement,
    DhAddonRequirement,
    GnomeCommonRequirement,
    GnulibDirectoryRequirement,
    GoPackageRequirement,
    IntrospectionTypelibRequirement,
    JavaClassRequirement,
    JavaScriptRuntimeRequirement,
    JDKFileRequirement,
    JDKRequirement,
    JRERequirement,
    LibraryRequirement,
    LibtoolRequirement,
    LuaModuleRequirement,
    NodeModuleRequirement,
    NodePackageRequirement,
    PathRequirement,
    PerlModuleRequirement,
    PhpClassRequirement,
    PHPExtensionRequirement,
    PkgConfigRequirement,
    PytestPluginRequirement,
    PythonModuleRequirement,
    PythonPackageRequirement,
    QtModuleRequirement,
    QTRequirement,
    Requirement,
    RPackageRequirement,
    RubyFileRequirement,
    RubyGemRequirement,
    SprocketsFileRequirement,
    StaticLibraryRequirement,
    VagueDependencyRequirement,
    ValaPackageRequirement,
    VcsControlDirectoryAccessRequirement,
    X11Requirement,
    XmlEntityRequirement,
)

ProblemToRequirementConverter = Callable[[Problem], Optional[Requirement]]


def map_pytest_arguments_to_plugin(args):
    for arg in args:
        if arg.startswith("--cov"):
            return PytestPluginRequirement("cov")
    return None


def map_pytest_config_option_to_plugin(name):
    if name == "asyncio_mode":
        return PytestPluginRequirement("asyncio")
    return None


# TODO(jelmer): populate this using an automated process
PYTEST_FIXTURE_TO_PLUGIN = {
    "aiohttp_client": "aiohttp",
    "aiohttp_client_cls": "aiohttp",
    "aiohttp_server": "aiohttp",
    "aiohttp_raw_server": "aiohttp",
    "mock": "mock",
    "benchmark": "benchmark",
    "event_loop": "asyncio",
    "unused_tcp_port": "asyncio",
    "unused_udp_port": "asyncio",
    "unused_tcp_port_factory": "asyncio",
    "unused_udp_port_factory": "asyncio",
}


def map_pytest_fixture_to_plugin(name):
    try:
        return PytestPluginRequirement(PYTEST_FIXTURE_TO_PLUGIN[name])
    except KeyError:
        return None


PROBLEM_CONVERTERS: list[
    Union[
        tuple[str, ProblemToRequirementConverter],
        tuple[str, ProblemToRequirementConverter, str],
    ]
] = [
    ("missing-file", lambda p: PathRequirement(p.path)),
    ("command-missing", lambda p: BinaryRequirement(p.command)),
    (
        "valac-cannot-compile",
        lambda p: VagueDependencyRequirement("valac"),
        "0.0.27",
    ),
    (
        "missing-cmake-files",
        lambda p: OneOfRequirement(
            [
                CMakefileRequirement(filename, p.version)
                for filename in p.filenames
            ]
        ),
    ),
    ("missing-command-or-build-file", lambda p: BinaryRequirement(p.command)),
    (
        "missing-pkg-config-package",
        lambda p: PkgConfigRequirement(p.module, p.minimum_version),
    ),
    ("missing-c-header", lambda p: CHeaderRequirement(p.header)),
    (
        "missing-introspection-typelib",
        lambda p: IntrospectionTypelibRequirement(p.library),
    ),
    (
        "missing-python-module",
        lambda p: PythonModuleRequirement(
            p.module,
            python_version=p.python_version,
            minimum_version=p.minimum_version,
        ),
    ),
    (
        "missing-python-distribution",
        lambda p: PythonPackageRequirement(
            p.distribution,
            python_version=p.python_version,
            minimum_version=p.minimum_version,
        ),
    ),
    ("javascript-runtime-missing", lambda p: JavaScriptRuntimeRequirement()),
    ("missing-node-module", lambda p: NodeModuleRequirement(p.module)),
    ("missing-node-package", lambda p: NodePackageRequirement(p.package)),
    ("missing-ruby-gem", lambda p: RubyGemRequirement(p.gem, p.version)),
    (
        "missing-qt-modules",
        lambda p: QtModuleRequirement(p.modules[0]),
        "0.0.27",
    ),
    ("missing-php-class", lambda p: PhpClassRequirement(p.php_class)),
    (
        "missing-r-package",
        lambda p: RPackageRequirement(p.package, p.minimum_version),
    ),
    (
        "missing-vague-dependency",
        lambda p: VagueDependencyRequirement(
            p.name, minimum_version=p.minimum_version
        ),
    ),
    ("missing-c#-compiler", lambda p: BinaryRequirement("msc")),
    ("missing-gnome-common", lambda p: GnomeCommonRequirement()),
    ("missing-jdk", lambda p: JDKRequirement()),
    ("missing-jre", lambda p: JRERequirement()),
    ("missing-qt", lambda p: QTRequirement()),
    ("missing-x11", lambda p: X11Requirement()),
    ("missing-libtool", lambda p: LibtoolRequirement()),
    ("missing-php-extension", lambda p: PHPExtensionRequirement(p.extension)),
    ("missing-rust-compiler", lambda p: BinaryRequirement("rustc")),
    ("missing-java-class", lambda p: JavaClassRequirement(p.classname)),
    ("missing-go-package", lambda p: GoPackageRequirement(p.package)),
    ("missing-autoconf-macro", lambda p: AutoconfMacroRequirement(p.macro)),
    ("missing-vala-package", lambda p: ValaPackageRequirement(p.package)),
    ("missing-lua-module", lambda p: LuaModuleRequirement(p.module)),
    ("missing-jdk-file", lambda p: JDKFileRequirement(p.jdk_path, p.filename)),
    ("missing-ruby-file", lambda p: RubyFileRequirement(p.filename)),
    ("missing-library", lambda p: LibraryRequirement(p.library)),
    (
        "missing-sprockets-file",
        lambda p: SprocketsFileRequirement(p.content_type, p.name),
    ),
    ("dh-addon-load-failure", lambda p: DhAddonRequirement(p.path)),
    ("missing-xml-entity", lambda p: XmlEntityRequirement(p.url)),
    (
        "missing-gnulib-directory",
        lambda p: GnulibDirectoryRequirement(p.directory),
    ),
    (
        "vcs-control-directory-needed",
        lambda p: VcsControlDirectoryAccessRequirement(p.vcs),
    ),
    (
        "missing-static-library",
        lambda p: StaticLibraryRequirement(p.library, p.filename),
    ),
    (
        "missing-perl-module",
        lambda p: PerlModuleRequirement(
            module=p.module, filename=p.filename, inc=p.inc
        ),
    ),
    (
        "unknown-certificate-authority",
        lambda p: CertificateAuthorityRequirement(p.url),
    ),
    (
        "unsupported-pytest-arguments",
        lambda p: map_pytest_arguments_to_plugin(p.args),
        "0.0.27",
    ),
    (
        "unsupported-pytest-config-option",
        lambda p: map_pytest_config_option_to_plugin(p.name),
        "0.0.34",
    ),
    (
        "missing-pytest-fixture",
        lambda p: map_pytest_fixture_to_plugin(p.fixture),
    ),
]

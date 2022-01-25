#!/usr/bin/python
# Copyright (C) 2019-2020 Jelmer Vernooij <jelmer@jelmer.uk>
# encoding: utf-8
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

import posixpath
import re
import subprocess
from typing import Optional, List, Set

from . import Requirement


class PythonPackageRequirement(Requirement):

    package: str

    def __init__(self, package, python_version=None, specs=None, minimum_version=None):
        super(PythonPackageRequirement, self).__init__("python-package")
        self.package = package
        self.python_version = python_version
        if minimum_version is not None:
            specs = [(">=", minimum_version)]
        if specs is None:
            specs = []
        self.specs = specs

    def __repr__(self):
        return "%s(%r, python_version=%r, specs=%r)" % (
            type(self).__name__,
            self.package,
            self.python_version,
            self.specs,
        )

    def __str__(self):
        if self.specs:
            return "python package: %s (%r)" % (self.package, self.specs)
        else:
            return "python package: %s" % (self.package,)

    @classmethod
    def from_requirement_str(cls, text):
        from requirements.requirement import Requirement

        req = Requirement.parse(text)
        return cls(package=req.name, specs=req.specs)

    def met(self, session):
        if self.python_version == "cpython3":
            cmd = "python3"
        elif self.python_version == "cpython2":
            cmd = "python2"
        elif self.python_version == "pypy":
            cmd = "pypy"
        elif self.python_version == "pypy3":
            cmd = "pypy3"
        elif self.python_version is None:
            cmd = "python3"
        else:
            raise NotImplementedError
        text = self.package + ",".join(["".join(spec) for spec in self.specs])
        p = session.Popen(
            [cmd, "-c", "import pkg_resources; pkg_resources.require(%r)" % text],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        p.communicate()
        return p.returncode == 0


class LatexPackageRequirement(Requirement):

    def __init__(self, package: str):
        self.package = package

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.package)


class PhpPackageRequirement(Requirement):
    def __init__(
        self,
        package: str,
        channel: Optional[str] = None,
        min_version: Optional[str] = None,
        max_version: Optional[str] = None,
    ):
        self.package = package
        self.channel = channel
        self.min_version = min_version
        self.max_version = max_version

    def __repr__(self):
        return "%s(%r, %r, %r, %r)" % (
            type(self).__name__,
            self.package,
            self.channel,
            self.min_version,
            self.max_version,
        )


class BinaryRequirement(Requirement):

    binary_name: str

    def __init__(self, binary_name):
        super(BinaryRequirement, self).__init__("binary")
        self.binary_name = binary_name

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.binary_name)

    def met(self, session):
        p = session.Popen(
            ["which", self.binary_name],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        p.communicate()
        return p.returncode == 0


class PHPExtensionRequirement(Requirement):

    extension: str

    def __init__(self, extension: str):
        super(PHPExtensionRequirement, self).__init__("php-extension")
        self.extension = extension

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.extension)


class PerlModuleRequirement(Requirement):

    module: str
    filename: Optional[str]
    inc: Optional[List[str]]

    def __init__(self, module, filename=None, inc=None):
        super(PerlModuleRequirement, self).__init__("perl-module")
        self.module = module
        self.filename = filename
        self.inc = inc

    @property
    def relfilename(self):
        return self.module.replace("::", "/") + ".pm"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.module)


class VagueDependencyRequirement(Requirement):

    name: str
    minimum_version: Optional[str] = None

    def __init__(self, name, minimum_version=None):
        super(VagueDependencyRequirement, self).__init__("vague")
        self.name = name
        self.minimum_version = minimum_version

    def expand(self):
        if " " not in self.name:
            yield BinaryRequirement(self.name)
            yield LibraryRequirement(self.name)
            yield PkgConfigRequirement(self.name, minimum_version=self.minimum_version)
            if self.name.lower() != self.name:
                yield BinaryRequirement(self.name.lower())
                yield LibraryRequirement(self.name.lower())
                yield PkgConfigRequirement(self.name.lower(), minimum_version=self.minimum_version)
            try:
                from .resolver.apt import AptRequirement
            except ModuleNotFoundError:
                pass
            else:
                yield AptRequirement.simple(self.name.lower(), minimum_version=self.minimum_version)
                if self.name.lower().startswith('lib'):
                    devname = '%s-dev' % self.name.lower()
                else:
                    devname = 'lib%s-dev' % self.name.lower()
                yield AptRequirement.simple(devname, minimum_version=self.minimum_version)

    def met(self, session):
        for x in self.expand():
            if x.met(session):
                return True
        return False

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.name)


class NodePackageRequirement(Requirement):

    package: str

    def __init__(self, package):
        super(NodePackageRequirement, self).__init__("npm-package")
        self.package = package

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.package)


class LuaModuleRequirement(Requirement):

    module: str

    def __init__(self, module):
        super(LuaModuleRequirement, self).__init__("lua-module")
        self.module = module

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.module)


class PerlPreDeclaredRequirement(Requirement):

    name: str

    # TODO(jelmer): Can we obtain this information elsewhere?
    KNOWN_MODULES = {
        'auto_set_repository': 'Module::Install::Repository',
        'author_tests': 'Module::Install::AuthorTests',
        'recursive_author_tests': 'Module::Install::AuthorTests',
        'author_requires': 'Module::Install::AuthorRequires',
        'readme_from': 'Module::Install::ReadmeFromPod',
        'catalyst': 'Module::Install::Catalyst',
        'githubmeta': 'Module::Install::GithubMeta',
        'use_ppport': 'Module::Install::XSUtil',
        'pod_from': 'Module::Install::PodFromEuclid',
        'write_doap_changes': 'Module::Install::DOAPChangeSets',
        'use_test_base': 'Module::Install::TestBase',
        'jsonmeta': 'Module::Install::JSONMETA',
        'extra_tests': 'Module::Install::ExtraTests',
        'auto_set_bugtracker': 'Module::Install::Bugtracker',
        }

    def __init__(self, name):
        super(PerlPreDeclaredRequirement, self).__init__("perl-predeclared")
        self.name = name

    def lookup_module(self):
        module = self.KNOWN_MODULES[self.name]
        return PerlModuleRequirement(module=module)

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.name)


class NodeModuleRequirement(Requirement):

    module: str

    def __init__(self, module):
        super(NodeModuleRequirement, self).__init__("npm-module")
        self.module = module

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.module)


class CargoCrateRequirement(Requirement):

    crate: str
    features: Set[str]
    version: Optional[str]

    def __init__(self, crate, features=None, version=None):
        super(CargoCrateRequirement, self).__init__("cargo-crate")
        self.crate = crate
        if features is None:
            features = set()
        self.features = features
        self.version = version

    def __repr__(self):
        return "%s(%r, features=%r, version=%r)" % (
            type(self).__name__,
            self.crate,
            self.features,
            self.version,
        )

    def __str__(self):
        if self.features:
            return "cargo crate: %s %s (%s)" % (
                self.crate,
                self.version or "",
                ", ".join(sorted(self.features)),
            )
        else:
            return "cargo crate: %s %s" % (self.crate, self.version or "")


class PkgConfigRequirement(Requirement):

    module: str

    def __init__(self, module, minimum_version=None):
        super(PkgConfigRequirement, self).__init__("pkg-config")
        self.module = module
        self.minimum_version = minimum_version

    def __repr__(self):
        return "%s(%r, minimum_version=%r)" % (
            type(self).__name__, self.module, self.minimum_version)


class PathRequirement(Requirement):

    path: str

    def __init__(self, path):
        super(PathRequirement, self).__init__("path")
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)


class CHeaderRequirement(Requirement):

    header: str

    def __init__(self, header):
        super(CHeaderRequirement, self).__init__("c-header")
        self.header = header

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.header)


class JavaScriptRuntimeRequirement(Requirement):
    def __init__(self):
        super(JavaScriptRuntimeRequirement, self).__init__("javascript-runtime")


class ValaPackageRequirement(Requirement):

    package: str

    def __init__(self, package: str):
        super(ValaPackageRequirement, self).__init__("vala")
        self.package = package


class RubyGemRequirement(Requirement):

    gem: str
    minimum_version: Optional[str]

    def __init__(self, gem: str, minimum_version: Optional[str]):
        super(RubyGemRequirement, self).__init__("gem")
        self.gem = gem
        self.minimum_version = minimum_version


class GoPackageRequirement(Requirement):

    package: str
    version: Optional[str]

    def __init__(self, package: str, version: Optional[str] = None):
        super(GoPackageRequirement, self).__init__("go-package")
        self.package = package
        self.version = version

    def __repr__(self):
        return "%s(%r, version=%r)" % (
            type(self).__name__, self.package, self.version)

    def __str__(self):
        if self.version:
            return "go package: %s (= %s)" % (self.package, self.version)
        return "go package: %s" % self.package


class GoRequirement(Requirement):

    version: Optional[str]

    def __init__(self, version: Optional[str] = None):
        super(GoRequirement, self).__init__("go")
        self.version = version

    def __str__(self):
        return "go %s" % self.version


class DhAddonRequirement(Requirement):

    path: str

    def __init__(self, path: str):
        super(DhAddonRequirement, self).__init__("dh-addon")
        self.path = path


class PhpClassRequirement(Requirement):

    php_class: str

    def __init__(self, php_class: str):
        super(PhpClassRequirement, self).__init__("php-class")
        self.php_class = php_class


class RPackageRequirement(Requirement):

    package: str
    minimum_version: Optional[str]

    def __init__(self, package: str, minimum_version: Optional[str] = None):
        super(RPackageRequirement, self).__init__("r-package")
        self.package = package
        self.minimum_version = minimum_version

    def __repr__(self):
        return "%s(%r, minimum_version=%r)" % (
            type(self).__name__,
            self.package,
            self.minimum_version,
        )

    def __str__(self):
        if self.minimum_version:
            return "R package: %s (>= %s)" % (self.package, self.minimum_version)
        else:
            return "R package: %s" % (self.package,)

    @classmethod
    def from_str(cls, text):
        # TODO(jelmer): More complex parser
        m = re.fullmatch(r"(.*)\s+\(>=\s+(.*)\)", text)
        if m:
            return cls(m.group(1), m.group(2))
        m = re.fullmatch(r"([^ ]+)", text)
        if m:
            return cls(m.group(1))
        raise ValueError(text)


class OctavePackageRequirement(Requirement):

    package: str
    minimum_version: Optional[str]

    def __init__(self, package: str, minimum_version: Optional[str] = None):
        super(OctavePackageRequirement, self).__init__("octave-package")
        self.package = package
        self.minimum_version = minimum_version

    def __repr__(self):
        return "%s(%r, minimum_version=%r)" % (
            type(self).__name__,
            self.package,
            self.minimum_version,
        )

    def __str__(self):
        if self.minimum_version:
            return "Octave package: %s (>= %s)" % (self.package, self.minimum_version)
        else:
            return "Octave package: %s" % (self.package,)

    @classmethod
    def from_str(cls, text):
        # TODO(jelmer): More complex parser
        m = re.fullmatch(r"(.*)\s+\(>=\s+(.*)\)", text)
        if m:
            return cls(m.group(1), m.group(2))
        m = re.fullmatch(r"([^ ]+)", text)
        if m:
            return cls(m.group(1))
        raise ValueError(text)


class LibraryRequirement(Requirement):

    library: str

    def __init__(self, library: str):
        super(LibraryRequirement, self).__init__("lib")
        self.library = library


class StaticLibraryRequirement(Requirement):

    library: str
    filename: str

    def __init__(self, library: str, filename: str):
        super(StaticLibraryRequirement, self).__init__("static-lib")
        self.library = library
        self.filename = filename


class RubyFileRequirement(Requirement):

    filename: str

    def __init__(self, filename: str):
        super(RubyFileRequirement, self).__init__("ruby-file")
        self.filename = filename


class XmlEntityRequirement(Requirement):

    url: str

    def __init__(self, url: str):
        super(XmlEntityRequirement, self).__init__("xml-entity")
        self.url = url


class SprocketsFileRequirement(Requirement):

    content_type: str
    name: str

    def __init__(self, content_type: str, name: str):
        super(SprocketsFileRequirement, self).__init__("sprockets-file")
        self.content_type = content_type
        self.name = name


class JavaClassRequirement(Requirement):

    classname: str

    def __init__(self, classname: str):
        super(JavaClassRequirement, self).__init__("java-class")
        self.classname = classname


class CMakefileRequirement(Requirement):

    filename: str
    version: Optional[str]

    def __init__(self, filename: str, version=None):
        super(CMakefileRequirement, self).__init__("cmake-file")
        self.filename = filename
        self.version = version


class HaskellPackageRequirement(Requirement):

    package: str

    def __init__(self, package: str, specs=None):
        super(HaskellPackageRequirement, self).__init__("haskell-package")
        self.package = package
        self.specs = specs

    @classmethod
    def from_string(cls, text):
        parts = text.split()
        return cls(parts[0], specs=parts[1:])


class MavenArtifactRequirement(Requirement):

    group_id: str
    artifact_id: str
    version: Optional[str]
    kind: Optional[str]

    def __init__(self, group_id, artifact_id, version=None, kind=None):
        super(MavenArtifactRequirement, self).__init__("maven-artifact")
        self.group_id = group_id
        self.artifact_id = artifact_id
        self.version = version
        self.kind = kind

    def __str__(self):
        return "maven requirement: %s:%s:%s" % (
            self.group_id,
            self.artifact_id,
            self.version,
        )

    def __repr__(self):
        return "%s(group_id=%r, artifact_id=%r, version=%r, kind=%r)" % (
            type(self).__name__, self.group_id, self.artifact_id,
            self.version, self.kind)

    @classmethod
    def from_str(cls, text):
        return cls.from_tuple(text.split(":"))

    @classmethod
    def from_tuple(cls, parts):
        if len(parts) == 4:
            (group_id, artifact_id, kind, version) = parts
        elif len(parts) == 3:
            (group_id, artifact_id, version) = parts
            kind = "jar"
        elif len(parts) == 2:
            version = None
            (group_id, artifact_id) = parts
            kind = "jar"
        else:
            raise ValueError("invalid number of parts to artifact %r" % parts)
        return cls(group_id, artifact_id, version, kind)


class GnomeCommonRequirement(Requirement):
    def __init__(self):
        super(GnomeCommonRequirement, self).__init__("gnome-common")


class JDKFileRequirement(Requirement):

    jdk_path: str
    filename: str

    def __init__(self, jdk_path: str, filename: str):
        super(JDKFileRequirement, self).__init__("jdk-file")
        self.jdk_path = jdk_path
        self.filename = filename

    @property
    def path(self):
        return posixpath.join(self.jdk_path, self.filename)


class JDKRequirement(Requirement):
    def __init__(self):
        super(JDKRequirement, self).__init__("jdk")


class JRERequirement(Requirement):
    def __init__(self):
        super(JRERequirement, self).__init__("jre")


class QTRequirement(Requirement):
    def __init__(self):
        super(QTRequirement, self).__init__("qt")


class X11Requirement(Requirement):
    def __init__(self):
        super(X11Requirement, self).__init__("x11")


class CertificateAuthorityRequirement(Requirement):
    def __init__(self, url):
        super(CertificateAuthorityRequirement, self).__init__("ca-cert")
        self.url = url


class PerlFileRequirement(Requirement):

    filename: str

    def __init__(self, filename: str):
        super(PerlFileRequirement, self).__init__("perl-file")
        self.filename = filename


class AutoconfMacroRequirement(Requirement):

    macro: str

    def __init__(self, macro: str):
        super(AutoconfMacroRequirement, self).__init__("autoconf-macro")
        self.macro = macro


class LibtoolRequirement(Requirement):
    def __init__(self):
        super(LibtoolRequirement, self).__init__("libtool")


class IntrospectionTypelibRequirement(Requirement):
    def __init__(self, library):
        self.library = library


class PythonModuleRequirement(Requirement):

    module: str
    python_version: Optional[str]
    minimum_version: Optional[str]

    def __init__(self, module, python_version=None, minimum_version=None):
        super(PythonModuleRequirement, self).__init__("python-module")
        self.module = module
        self.python_version = python_version
        self.minimum_version = minimum_version

    def met(self, session):
        if self.python_version == "cpython3":
            cmd = "python3"
        elif self.python_version == "cpython2":
            cmd = "python2"
        elif self.python_version == "pypy":
            cmd = "pypy"
        elif self.python_version == "pypy3":
            cmd = "pypy3"
        elif self.python_version is None:
            cmd = "python3"
        else:
            raise NotImplementedError
        p = session.Popen(
            [cmd, "-c", "import %s" % self.module],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        p.communicate()
        return p.returncode == 0

    def __repr__(self):
        return "%s(%r, python_version=%r, minimum_version=%r)" % (
            type(self).__name__, self.module, self.python_version,
            self.minimum_version)


class BoostComponentRequirement(Requirement):

    name: str

    def __init__(self, name):
        super(BoostComponentRequirement, self).__init__("boost-component")
        self.name = name


class GnulibDirectoryRequirement(Requirement):

    directory: str

    def __init__(self, directory):
        super(GnulibDirectoryRequirement, self).__init__("gnulib")
        self.directory = directory

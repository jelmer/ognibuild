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

    family = "python-package"

    package: str

    def __init__(
            self, package, python_version=None, specs=None,
            minimum_version=None):
        self.package = package
        self.python_version = python_version
        if specs is None:
            specs = []
        if minimum_version is not None:
            specs.append((">=", minimum_version))
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
    def from_requirement_str(cls, text, python_version=None):
        from requirements.requirement import Requirement as RequirementEntry

        req = RequirementEntry.parse(text)
        return cls(
            package=req.name, specs=req.specs, python_version=python_version)

    def requirement_str(self):
        if self.specs:
            return '%s;%s' % (
                self.package, ','.join([''.join(s) for s in self.specs]))
        return self.package

    @classmethod
    def _from_json(cls, js):
        if isinstance(js, str):
            return cls.from_requirement_str(js)
        return cls.from_requirement_str(js[0], python_version=js[1])

    def _json(self):
        if self.python_version:
            return [self.requirement_str(), self.python_version]
        return self.requirement_str()

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
            [cmd, "-c",
             "import pkg_resources; pkg_resources.require(%r)" % text],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        p.communicate()
        return p.returncode == 0


Requirement.register_json(PythonPackageRequirement)


class LatexPackageRequirement(Requirement):

    family = "latex-package"

    def __init__(self, package: str):
        self.package = package

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.package)

    def _json(self):
        return self.package

    @classmethod
    def _from_json(cls, package):
        return cls(package)


Requirement.register_json(LatexPackageRequirement)


class PhpPackageRequirement(Requirement):

    family = "php-package"

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

    def _json(self):
        return (self.package, self.channel, self.min_version, self.max_version)

    @classmethod
    def _from_json(cls, js):
        return cls(*js)

    def __repr__(self):
        return "%s(%r, %r, %r, %r)" % (
            type(self).__name__,
            self.package,
            self.channel,
            self.min_version,
            self.max_version,
        )


Requirement.register_json(PhpPackageRequirement)


class BinaryRequirement(Requirement):

    family = "binary"
    binary_name: str

    def __init__(self, binary_name):
        self.binary_name = binary_name

    def _json(self):
        return self.binary_name

    @classmethod
    def _from_json(cls, js):
        return cls(js)

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


Requirement.register_json(BinaryRequirement)


class PHPExtensionRequirement(Requirement):

    family = "php-extension"
    extension: str

    def __init__(self, extension: str):
        self.extension = extension

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.extension)


class PytestPluginRequirement(Requirement):

    family = "pytest-plugin"

    plugin: str

    def __init__(self, plugin: str):
        self.plugin = plugin

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.plugin)


class VcsControlDirectoryAccessRequirement(Requirement):

    vcs: List[str]
    family = "vcs-access"

    def __init__(self, vcs):
        self.vcs = vcs

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.vcs)


class PerlModuleRequirement(Requirement):

    module: str
    filename: Optional[str]
    inc: Optional[List[str]]
    family = "perl-module"

    def __init__(self, module, filename=None, inc=None):
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
    family = "vague"
    minimum_version: Optional[str] = None

    def __init__(self, name, minimum_version=None):
        self.name = name
        self.minimum_version = minimum_version

    def expand(self):
        if " " not in self.name:
            yield BinaryRequirement(self.name)
            yield LibraryRequirement(self.name)
            yield PkgConfigRequirement(
                self.name, minimum_version=self.minimum_version)
            if self.name.lower() != self.name:
                yield BinaryRequirement(self.name.lower())
                yield LibraryRequirement(self.name.lower())
                yield PkgConfigRequirement(
                    self.name.lower(), minimum_version=self.minimum_version)
            try:
                from .resolver.apt import AptRequirement
            except ModuleNotFoundError:
                pass
            else:
                yield AptRequirement.simple(
                    self.name.lower(), minimum_version=self.minimum_version)
                if self.name.lower().startswith('lib'):
                    devname = '%s-dev' % self.name.lower()
                else:
                    devname = 'lib%s-dev' % self.name.lower()
                yield AptRequirement.simple(
                    devname, minimum_version=self.minimum_version)

    def met(self, session):
        return any(x.met(session) for x in self.expand())

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.name)

    def __str__(self):
        if self.minimum_version:
            return "%s >= %s" % (self.name, self.minimum_version)
        return self.name


class NodePackageRequirement(Requirement):

    package: str
    family = "npm-package"

    def __init__(self, package):
        self.package = package

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.package)


class LuaModuleRequirement(Requirement):

    module: str
    family = "lua-module"

    def __init__(self, module):
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

    family = "perl-predeclared"

    def __init__(self, name):
        self.name = name

    def lookup_module(self):
        module = self.KNOWN_MODULES[self.name]
        return PerlModuleRequirement(module=module)

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.name)


class NodeModuleRequirement(Requirement):

    module: str
    family = "npm-module"

    def __init__(self, module):
        self.module = module

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.module)


class CargoCrateRequirement(Requirement):

    crate: str
    features: Set[str]
    api_version: Optional[str]
    minimum_version: Optional[str]
    family = "cargo-crate"

    def __init__(self, crate, features=None, api_version=None,
                 minimum_version=None):
        self.crate = crate
        if features is None:
            features = set()
        self.features = features
        self.api_version = api_version
        self.minimum_version = minimum_version

    def __repr__(self):
        return "%s(%r, features=%r, api_version=%r, minimum_version=%r)" % (
            type(self).__name__,
            self.crate,
            self.features,
            self.api_version,
            self.minimum_version,
        )

    def __str__(self):
        ret = "cargo crate: %s %s" % (
            self.crate,
            self.api_version or "")
        if self.features:
            ret += " (%s)" % (", ".join(sorted(self.features)))
        if self.minimum_version:
            ret += " (>= %s)" % self.minimum_version
        return ret


class PkgConfigRequirement(Requirement):

    module: str
    family = "pkg-config"

    def __init__(self, module, minimum_version=None):
        self.module = module
        self.minimum_version = minimum_version

    def __repr__(self):
        return "%s(%r, minimum_version=%r)" % (
            type(self).__name__, self.module, self.minimum_version)


class PathRequirement(Requirement):

    path: str
    family = "path"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)


class CHeaderRequirement(Requirement):

    header: str
    family = "c-header"

    def __init__(self, header):
        self.header = header

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.header)


class JavaScriptRuntimeRequirement(Requirement):
    family = "javascript-runtime"


class ValaPackageRequirement(Requirement):

    package: str
    family = "vala"

    def __init__(self, package: str):
        self.package = package


class RubyGemRequirement(Requirement):

    gem: str
    minimum_version: Optional[str]
    family = "gem"

    def __init__(self, gem: str, minimum_version: Optional[str]):
        self.gem = gem
        self.minimum_version = minimum_version


class GoPackageRequirement(Requirement):

    package: str
    version: Optional[str]
    family = "go-package"

    def __init__(self, package: str, version: Optional[str] = None):
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
    family = "go"

    def __init__(self, version: Optional[str] = None):
        self.version = version

    def __str__(self):
        return "go %s" % self.version


class DhAddonRequirement(Requirement):

    path: str
    family = "dh-addon"

    def __init__(self, path: str):
        self.path = path


class PhpClassRequirement(Requirement):

    php_class: str
    family = "php-class"

    def __init__(self, php_class: str):
        self.php_class = php_class


class RPackageRequirement(Requirement):

    package: str
    minimum_version: Optional[str]
    family = "r-package"

    def __init__(self, package: str, minimum_version: Optional[str] = None):
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
            return "R package: %s (>= %s)" % (
                self.package, self.minimum_version)
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
    family = "octave-package"

    def __init__(self, package: str, minimum_version: Optional[str] = None):
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
            return "Octave package: %s (>= %s)" % (
                self.package, self.minimum_version)
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
    family = "lib"

    def __init__(self, library: str):
        self.library = library

    def __str__(self):
        return "Library: %s" % self.library


class StaticLibraryRequirement(Requirement):

    library: str
    filename: str
    family = "static-lib"

    def __init__(self, library: str, filename: str):
        self.library = library
        self.filename = filename

    def __str__(self):
        return "Static Library: %s" % self.library


class RubyFileRequirement(Requirement):

    filename: str
    family = "ruby-file"

    def __init__(self, filename: str):
        self.filename = filename


class XmlEntityRequirement(Requirement):

    url: str
    family = "xml-entity"

    def __init__(self, url: str):
        self.url = url


class SprocketsFileRequirement(Requirement):

    content_type: str
    name: str
    family = "sprockets-file"

    def __init__(self, content_type: str, name: str):
        self.content_type = content_type
        self.name = name


class JavaClassRequirement(Requirement):

    classname: str
    family = "java-class"

    def __init__(self, classname: str):
        self.classname = classname


class CMakefileRequirement(Requirement):

    filename: str
    version: Optional[str]
    family = "cmake-file"

    def __init__(self, filename: str, version=None):
        self.filename = filename
        self.version = version


class HaskellPackageRequirement(Requirement):

    package: str
    family = "haskell-package"

    def __init__(self, package: str, specs=None):
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
    family = "maven-artifact"

    def __init__(self, group_id, artifact_id, version=None, kind=None):
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
    family = "gnome-common"


class JDKFileRequirement(Requirement):

    jdk_path: str
    filename: str
    family = "jdk-file"

    def __init__(self, jdk_path: str, filename: str):
        self.jdk_path = jdk_path
        self.filename = filename

    @property
    def path(self):
        return posixpath.join(self.jdk_path, self.filename)


class JDKRequirement(Requirement):
    family = "jdk"


class JRERequirement(Requirement):
    family = "jre"


class QtModuleRequirement(Requirement):
    family = "qt-module"

    def __init__(self, module):
        self.module = module


class QTRequirement(Requirement):
    family = "qt"


class X11Requirement(Requirement):
    family = "x11"


class CertificateAuthorityRequirement(Requirement):
    family = "ca-cert"

    def __init__(self, url):
        self.url = url


class PerlFileRequirement(Requirement):

    filename: str
    family = "perl-file"

    def __init__(self, filename: str):
        self.filename = filename


class AutoconfMacroRequirement(Requirement):

    family = "autoconf-macro"
    macro: str

    def __init__(self, macro: str):
        self.macro = macro

    def _json(self):
        return self.macro

    @classmethod
    def _from_json(cls, macro):
        return cls(macro)


Requirement.register_json(AutoconfMacroRequirement)


class LibtoolRequirement(Requirement):
    family = "libtool"


class IntrospectionTypelibRequirement(Requirement):
    family = "introspection-type-lib"

    def __init__(self, library):
        self.library = library


class PythonModuleRequirement(Requirement):

    module: str
    python_version: Optional[str]
    minimum_version: Optional[str]
    family = "python-module"

    def __init__(self, module, python_version=None, minimum_version=None):
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
    family = "boost-component"

    def __init__(self, name):
        self.name = name


class KF5ComponentRequirement(Requirement):

    name: str
    family = "kf5-component"

    def __init__(self, name):
        self.name = name


class GnulibDirectoryRequirement(Requirement):

    directory: str
    family = "gnulib"

    def __init__(self, directory):
        self.directory = directory

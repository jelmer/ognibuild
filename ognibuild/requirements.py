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
import subprocess
from typing import Optional, List, Tuple

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
        text = self.package + ','.join([''.join(spec) for spec in self.specs])
        p = session.Popen(
            [cmd, "-c", "import pkg_resources; pkg_resources.require(%r)" % text],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        p.communicate()
        return p.returncode == 0


class BinaryRequirement(Requirement):

    binary_name: str

    def __init__(self, binary_name):
        super(BinaryRequirement, self).__init__("binary")
        self.binary_name = binary_name

    def met(self, session):
        p = session.Popen(
            ["which", self.binary_name], stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL)
        p.communicate()
        return p.returncode == 0


class PerlModuleRequirement(Requirement):

    module: str
    filename: Optional[str]
    inc: Optional[List[str]]

    def __init__(self, module, filename=None, inc=None):
        super(PerlModuleRequirement, self).__init__("perl-module")
        self.module = module
        self.filename = filename
        self.inc = inc

    def relfilename(self):
        return self.module.replace("::", "/") + ".pm"


class NodePackageRequirement(Requirement):

    package: str

    def __init__(self, package):
        super(NodePackageRequirement, self).__init__("npm-package")
        self.package = package


class CargoCrateRequirement(Requirement):

    crate: str

    def __init__(self, crate):
        super(CargoCrateRequirement, self).__init__("cargo-crate")
        self.crate = crate

    def __repr__(self):
        return "%s(%r)" % (
            type(self).__name__,
            self.crate,
        )

    def __str__(self):
        return "cargo crate: %s" % self.crate


class PkgConfigRequirement(Requirement):

    module: str

    def __init__(self, module, minimum_version=None):
        super(PkgConfigRequirement, self).__init__("pkg-config")
        self.module = module
        self.minimum_version = minimum_version


class PathRequirement(Requirement):

    path: str

    def __init__(self, path):
        super(PathRequirement, self).__init__("path")
        self.path = path


class CHeaderRequirement(Requirement):

    header: str

    def __init__(self, header):
        super(CHeaderRequirement, self).__init__("c-header")
        self.header = header


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

    def __init__(self, package: str):
        super(GoPackageRequirement, self).__init__("go")
        self.package = package


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


class LibraryRequirement(Requirement):

    library: str

    def __init__(self, library: str):
        super(LibraryRequirement, self).__init__("lib")
        self.library = library


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

    artifacts: List[Tuple[str, str, str]]

    def __init__(self, artifacts):
        super(MavenArtifactRequirement, self).__init__("maven-artifact")
        self.artifacts = artifacts


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


class PythonModuleRequirement(Requirement):

    module: str
    python_version: Optional[str]
    minimum_version: Optional[str]

    def __init__(self, module, python_version=None, minimum_version=None):
        super(PythonModuleRequirement, self).__init__("python-module")
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
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        p.communicate()
        return p.returncode == 0

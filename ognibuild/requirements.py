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
from typing import Optional, List, Tuple

from . import UpstreamRequirement


class PythonPackageRequirement(UpstreamRequirement):

    package: str

    def __init__(self, package):
        super(PythonPackageRequirement, self).__init__('python-package')
        self.package = package


class BinaryRequirement(UpstreamRequirement):

    binary_name: str

    def __init__(self, binary_name):
        super(BinaryRequirement, self).__init__('binary')
        self.binary_name = binary_name


class PerlModuleRequirement(UpstreamRequirement):

    module: str
    filename: Optional[str]
    inc: Optional[List[str]]

    def __init__(self, module, filename=None, inc=None):
        super(PerlModuleRequirement, self).__init__('perl-module')
        self.module = module
        self.filename = filename
        self.inc = inc

    def relfilename(self):
        return self.module.replace("::", "/") + ".pm"


class NodePackageRequirement(UpstreamRequirement):

    package: str

    def __init__(self, package):
        super(NodePackageRequirement, self).__init__('npm-package')
        self.package = package


class CargoCrateRequirement(UpstreamRequirement):

    crate: str

    def __init__(self, crate):
        super(CargoCrateRequirement, self).__init__('cargo-crate')
        self.crate = crate


class PkgConfigRequirement(UpstreamRequirement):

    module: str

    def __init__(self, module, minimum_version=None):
        super(PkgConfigRequirement, self).__init__('pkg-config')
        self.module = module
        self.minimum_version = minimum_version


class PathRequirement(UpstreamRequirement):

    path: str

    def __init__(self, path):
        super(PathRequirement, self).__init__('path')
        self.path = path


class CHeaderRequirement(UpstreamRequirement):

    header: str

    def __init__(self, header):
        super(CHeaderRequirement, self).__init__('c-header')
        self.header = header


class JavaScriptRuntimeRequirement(UpstreamRequirement):

    def __init__(self):
        super(JavaScriptRuntimeRequirement, self).__init__(
            'javascript-runtime')


class ValaPackageRequirement(UpstreamRequirement):

    package: str

    def __init__(self, package: str):
        super(ValaPackageRequirement, self).__init__('vala')
        self.package = package


class RubyGemRequirement(UpstreamRequirement):

    gem: str
    minimum_version: Optional[str]

    def __init__(self, gem: str, minimum_version: Optional[str]):
        super(RubyGemRequirement, self).__init__('gem')
        self.gem = gem
        self.minimum_version = minimum_version


class GoPackageRequirement(UpstreamRequirement):

    package: str

    def __init__(self, package: str):
        super(GoPackageRequirement, self).__init__('go')
        self.package = package


class DhAddonRequirement(UpstreamRequirement):

    path: str

    def __init__(self, path: str):
        super(DhAddonRequirement, self).__init__('dh-addon')
        self.path = path


class PhpClassRequirement(UpstreamRequirement):

    php_class: str

    def __init__(self, php_class: str):
        super(PhpClassRequirement, self).__init__('php-class')
        self.php_class = php_class


class RPackageRequirement(UpstreamRequirement):

    package: str
    minimum_version: Optional[str]

    def __init__(self, package: str, minimum_version: Optional[str] = None):
        super(RPackageRequirement, self).__init__('r-package')
        self.package = package
        self.minimum_version = minimum_version


class LibraryRequirement(UpstreamRequirement):

    library: str

    def __init__(self, library: str):
        super(LibraryRequirement, self).__init__('lib')
        self.library = library


class RubyFileRequirement(UpstreamRequirement):

    filename: str

    def __init__(self, filename: str):
        super(RubyFileRequirement, self).__init__('ruby-file')
        self.filename = filename


class XmlEntityRequirement(UpstreamRequirement):

    url: str

    def __init__(self, url: str):
        super(XmlEntityRequirement, self).__init__('xml-entity')
        self.url = url


class SprocketsFileRequirement(UpstreamRequirement):

    content_type: str
    name: str

    def __init__(self, content_type: str, name: str):
        super(SprocketsFileRequirement, self).__init__('sprockets-file')
        self.content_type = content_type
        self.name = name


class JavaClassRequirement(UpstreamRequirement):

    classname: str

    def __init__(self, classname: str):
        super(JavaClassRequirement, self).__init__('java-class')
        self.classname = classname


class HaskellPackageRequirement(UpstreamRequirement):

    package: str

    def __init__(self, package: str):
        super(HaskellPackageRequirement, self).__init__('haskell-package')
        self.package = package


class MavenArtifactRequirement(UpstreamRequirement):

    artifacts: List[Tuple[str, str, str]]

    def __init__(self, artifacts):
        super(MavenArtifactRequirement, self).__init__('maven-artifact')
        self.artifacts = artifacts


class GnomeCommonRequirement(UpstreamRequirement):

    def __init__(self):
        super(GnomeCommonRequirement, self).__init__('gnome-common')


class JDKFileRequirement(UpstreamRequirement):

    jdk_path: str
    filename: str

    def __init__(self, jdk_path: str, filename: str):
        super(JDKFileRequirement, self).__init__('jdk-file')
        self.jdk_path = jdk_path
        self.filename = filename

    @property
    def path(self):
        return posixpath.join(self.jdk_path, self.filename)


class PerlFileRequirement(UpstreamRequirement):

    filename: str

    def __init__(self, filename: str):
        super(PerlFileRequirement, self).__init__('perl-file')
        self.filename = filename


class AutoconfMacroRequirement(UpstreamRequirement):

    macro: str

    def __init__(self, macro: str):
        super(AutoconfMacroRequirement, self).__init__('autoconf-macro')
        self.macro = macro

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

import logging
import os
import posixpath

from ..debian.apt import AptManager

from . import Resolver, UnsatisfiedRequirements
from ..requirements import (
    BinaryRequirement,
    CHeaderRequirement,
    PkgConfigRequirement,
    PathRequirement,
    UpstreamRequirement,
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


class AptRequirement(object):

    def __init__(self, package, minimum_version=None):
        self.package = package
        self.minimum_version = minimum_version


def get_package_for_python_package(apt_mgr, package, python_version, minimum_version=None):
    if python_version == "pypy":
        pkg_name = apt_mgr.get_package_for_paths(
            ["/usr/lib/pypy/dist-packages/%s-.*.egg-info/PKG-INFO" % package],
            regex=True)
    elif python_version == "cpython2":
        pkg_name = apt_mgr.get_package_for_paths(
            ["/usr/lib/python2\\.[0-9]/dist-packages/%s-.*.egg-info/PKG-INFO" % package],
            regex=True)
    elif python_version == "cpython3":
        pkg_name = apt_mgr.get_package_for_paths(
            ["/usr/lib/python3/dist-packages/%s-.*.egg-info/PKG-INFO" % package],
            regex=True)
    else:
        raise NotImplementedError
    # TODO(jelmer): Dealing with epoch, etc?
    if pkg_name is not None:
        return AptRequirement(pkg_name, minimum_version)
    return None


def get_package_for_python_module(apt_mgr, module, python_version, minimum_version):
    if python_version == "python3":
        paths = [
            posixpath.join(
                "/usr/lib/python3/dist-packages",
                module.replace(".", "/"),
                "__init__.py",
            ),
            posixpath.join(
                "/usr/lib/python3/dist-packages", module.replace(".", "/") + ".py"
            ),
            posixpath.join(
                "/usr/lib/python3\\.[0-9]+/lib-dynload",
                module.replace(".", "/") + "\\.cpython-.*\\.so",
            ),
            posixpath.join(
                "/usr/lib/python3\\.[0-9]+/", module.replace(".", "/") + ".py"
            ),
            posixpath.join(
                "/usr/lib/python3\\.[0-9]+/", module.replace(".", "/"), "__init__.py"
            ),
        ]
    elif python_version == "python2":
        paths = [
            posixpath.join(
                "/usr/lib/python2\\.[0-9]/dist-packages",
                module.replace(".", "/"),
                "__init__.py",
            ),
            posixpath.join(
                "/usr/lib/python2\\.[0-9]/dist-packages",
                module.replace(".", "/") + ".py",
            ),
            posixpath.join(
                "/usr/lib/python2.\\.[0-9]/lib-dynload",
                module.replace(".", "/") + ".so",
            ),
        ]
    elif python_version == "pypy":
        paths = [
            posixpath.join(
                "/usr/lib/pypy/dist-packages", module.replace(".", "/"), "__init__.py"
            ),
            posixpath.join(
                "/usr/lib/pypy/dist-packages", module.replace(".", "/") + ".py"
            ),
            posixpath.join(
                "/usr/lib/pypy/dist-packages",
                module.replace(".", "/") + "\\.pypy-.*\\.so",
            ),
        ]
    else:
        raise AssertionError("unknown python version %r" % python_version)
    pkg_name = apt_mgr.get_package_for_paths(paths, regex=True)
    if pkg_name is not None:
        return AptRequirement(pkg_name, minimum_version=minimum_version)
    return None


def resolve_binary_req(apt_mgr, req):
    if posixpath.isabs(req.binary_name):
        paths = [req.binary_name]
    else:
        paths = [
            posixpath.join(dirname, req.binary_name)
            for dirname in ["/usr/bin", "/bin"]
        ]
    pkg_name = apt_mgr.get_package_for_paths(paths)
    if pkg_name is not None:
        return AptRequirement(pkg_name)
    return None


def resolve_pkg_config_req(apt_mgr, req):
    package = apt_mgr.get_package_for_paths(
        [posixpath.join("/usr/lib/pkgconfig", req.module + ".pc")],
    )
    if package is None:
        package = apt_mgr.get_package_for_paths(
            [posixpath.join("/usr/lib", ".*", "pkgconfig", req.module + ".pc")],
            regex=True)
    if package is not None:
        return AptRequirement(package, minimum_version=req.minimum_version)
    return None


def resolve_path_req(apt_mgr, req):
    package = apt_mgr.get_package_for_paths([req.path])
    if package is not None:
        return AptRequirement(package)
    return None


def resolve_c_header_req(apt_mgr, req):
    package = apt_mgr.get_package_for_paths(
        [posixpath.join("/usr/include", req.header)], regex=False
    )
    if package is None:
        package = apt_mgr.get_package_for_paths(
            [posixpath.join("/usr/include", ".*", req.header)], regex=True
        )
    if package is None:
        return None
    return AptRequirement(package)


def resolve_js_runtime_req(apt_mgr, req):
    package = apt_mgr.get_package_for_paths(
        ["/usr/bin/node", "/usr/bin/duk"], regex=False)
    if package is not None:
        return AptRequirement(package)
    return None


def resolve_vala_package_req(apt_mgr, req):
    path = "/usr/share/vala-[0-9.]+/vapi/%s.vapi" % req.package
    package = apt_mgr.get_package_for_paths([path], regex=True)
    if package is not None:
        return AptRequirement(package)
    return None


def resolve_ruby_gem_req(apt_mgr, req):
    paths = [
        posixpath.join(
            "/usr/share/rubygems-integration/all/"
            "specifications/%s-.*\\.gemspec" % req.gem
        )
    ]
    package = apt_mgr.get_package_for_paths(
        paths, regex=True)
    if package is not None:
        return AptRequirement(package, minimum_version=req.minimum_version)
    return None


def resolve_go_package_req(apt_mgr, req):
    package = apt_mgr.get_package_for_paths(
        [posixpath.join("/usr/share/gocode/src", req.package, ".*")],
        regex=True
    )
    if package is not None:
        return AptRequirement(package)
    return None


def resolve_dh_addon_req(apt_mgr, req):
    paths = [posixpath.join("/usr/share/perl5", req.path)]
    package = apt_mgr.get_package_for_paths(paths)
    if package is not None:
        return AptRequirement(package)
    return None


def resolve_php_class_req(apt_mgr, req):
    path = "/usr/share/php/%s.php" % req.php_class.replace("\\", "/")
    package = apt_mgr.get_package_for_paths([path])
    if package is not None:
        return AptRequirement(package)
    return None


def resolve_r_package_req(apt_mgr, req):
    paths = [posixpath.join("/usr/lib/R/site-library/.*/R/%s$" % req.package)]
    package = apt_mgr.get_package_for_paths(paths, regex=True)
    if package is not None:
        return AptRequirement(package)
    return None


def resolve_node_package_req(apt_mgr, req):
    paths = [
        "/usr/share/nodejs/.*/node_modules/%s/package.json" % req.package,
        "/usr/lib/nodejs/%s/package.json" % req.package,
        "/usr/share/nodejs/%s/package.json" % req.package,
    ]
    pkg_name = apt_mgr.get_package_for_paths(paths, regex=True)
    if pkg_name is not None:
        return AptRequirement(pkg_name)
    return None


def resolve_library_req(apt_mgr, req):
    paths = [
        posixpath.join("/usr/lib/lib%s.so$" % req.library),
        posixpath.join("/usr/lib/.*/lib%s.so$" % req.library),
        posixpath.join("/usr/lib/lib%s.a$" % req.library),
        posixpath.join("/usr/lib/.*/lib%s.a$" % req.library),
    ]
    pkg_name = apt_mgr.get_package_for_paths(paths, regex=True)
    if pkg_name is not None:
        return AptRequirement(pkg_name)
    return None


def resolve_ruby_file_req(apt_mgr, req):
    paths = [posixpath.join("/usr/lib/ruby/vendor_ruby/%s.rb" % req.filename)]
    package = apt_mgr.get_package_for_paths(paths)
    if package is not None:
        return AptRequirement(package)
    paths = [
        posixpath.join(
            r"/usr/share/rubygems-integration/all/gems/([^/]+)/"
            "lib/%s.rb" % req.filename
        )
    ]
    pkg_name = apt_mgr.get_package_for_paths(paths, regex=True)
    if pkg_name is not None:
        return AptRequirement(pkg_name)
    return None


def resolve_xml_entity_req(apt_mgr, req):
    # Ideally we should be using the XML catalog for this, but hardcoding
    # a few URLs will do for now..
    URL_MAP = {
        "http://www.oasis-open.org/docbook/xml/": "/usr/share/xml/docbook/schema/dtd/"
    }
    for url, path in URL_MAP.items():
        if req.url.startswith(url):
            search_path = posixpath.join(path, req.url[len(url) :])
            break
    else:
        return None

    pkg_name = apt_mgr.get_package_for_paths([search_path], regex=False)
    if pkg_name is not None:
        return AptRequirement(pkg_name)
    return None


def resolve_sprockets_file_req(apt_mgr, req):
    if req.content_type == "application/javascript":
        path = "/usr/share/.*/app/assets/javascripts/%s.js$" % req.name
    else:
        logging.warning("unable to handle content type %s", req.content_type)
        return None
    pkg_name = apt_mgr.get_package_for_paths([path], regex=True)
    if pkg_name is not None:
        return AptRequirement(pkg_name)
    return None


def resolve_java_class_req(apt_mgr, req):
    # Unfortunately this only finds classes in jars installed on the host
    # system :(
    # TODO(jelmer): Call in session
    output = apt_mgr.session.check_output(
        ["java-propose-classpath", "-c" + req.classname])
    classpath = [p for p in output.decode().strip(":").strip().split(":") if p]
    if not classpath:
        logging.warning("unable to find classpath for %s", req.classname)
        return False
    logging.info("Classpath for %s: %r", req.classname, classpath)
    package = apt_mgr.get_package_for_paths(classpath)
    if package is None:
        logging.warning("no package for files in %r", classpath)
        return None
    return AptRequirement(package)


def resolve_haskell_package_req(apt_mgr, req):
    path = "/var/lib/ghc/package.conf.d/%s-.*.conf" % req.deps[0][0]
    pkg_name = apt_mgr.get_package_for_paths([path], regex=True)
    if pkg_name is not None:
        return AptRequirement(pkg_name)
    return None


def resolve_maven_artifact_req(apt_mgr, req):
    artifact = req.artifacts[0]
    parts = artifact.split(":")
    if len(parts) == 4:
        (group_id, artifact_id, kind, version) = parts
        regex = False
    elif len(parts) == 3:
        (group_id, artifact_id, version) = parts
        kind = "jar"
        regex = False
    elif len(parts) == 2:
        version = ".*"
        (group_id, artifact_id) = parts
        kind = "jar"
        regex = True
    else:
        raise AssertionError("invalid number of parts to artifact %s" % artifact)
    paths = [
        posixpath.join(
            "/usr/share/maven-repo",
            group_id.replace(".", "/"),
            artifact_id,
            version,
            "%s-%s.%s" % (artifact_id, version, kind),
        )
    ]
    pkg_name = apt_mgr.get_package_for_paths(paths, regex=regex)
    if pkg_name is not None:
        return AptRequirement(pkg_name)
    return None


def resolve_gnome_common_req(apt_mgr, req):
    return AptRequirement('gnome-common')


def resolve_jdk_file_req(apt_mgr, req):
    path = req.jdk_path + ".*/" + req.filename
    pkg_name = apt_mgr.get_package_for_paths([path], regex=True)
    if pkg_name is not None:
        return AptRequirement(pkg_name)
    return None


def resolve_perl_module_req(apt_mgr, req):
    DEFAULT_PERL_PATHS = ["/usr/share/perl5"]

    if req.inc is None:
        if req.filename is None:
            paths = [posixpath.join(inc, req.relfilename)
                     for inc in DEFAULT_PERL_PATHS]
        elif not posixpath.isabs(req.filename):
            return False
        else:
            paths = [req.filename]
    else:
        paths = [posixpath.join(inc, req.filename) for inc in req.inc]
    pkg_name = apt_mgr.get_package_for_paths(paths, regex=False)
    if pkg_name is not None:
        return AptRequirement(pkg_name)
    return None


def resolve_perl_file_req(apt_mgr, req):
    pkg_name = apt_mgr.get_package_for_paths([req.filename], regex=False)
    if pkg_name is not None:
        return AptRequirement(pkg_name)
    return None


def _find_aclocal_fun(macro):
    # TODO(jelmer): Use the API for codesearch.debian.net instead?
    defun_prefix = b"AC_DEFUN([%s]," % macro.encode("ascii")
    for entry in os.scandir("/usr/share/aclocal"):
        if not entry.is_file():
            continue
        with open(entry.path, "rb") as f:
            for line in f:
                if line.startswith(defun_prefix):
                    return entry.path
    raise KeyError


def resolve_autoconf_macro_req(apt_mgr, req):
    try:
        path = _find_aclocal_fun(req.macro)
    except KeyError:
        logging.info("No local m4 file found defining %s", req.macro)
        return None
    pkg_name = apt_mgr.get_package_for_paths([path])
    if pkg_name is not None:
        return AptRequirement(pkg_name)
    return None


def resolve_python_module_req(apt_mgr, req):
    if req.python_version == 2:
        return get_package_for_python_module(apt_mgr, req.module, "cpython2", req.minimum_version)
    elif req.python_version in (None, 3):
        return get_package_for_python_module(apt_mgr, req.module, "cpython3", req.minimum_version)
    else:
        return None


def resolve_python_package_req(apt_mgr, req):
    if req.python_version == 2:
        return get_package_for_python_package(apt_mgr, req.package, "cpython2", req.minimum_version)
    elif req.python_version in (None, 3):
        return get_package_for_python_package(apt_mgr, req.package, "cpython3", req.minimum_version)
    else:
        return None


APT_REQUIREMENT_RESOLVERS = [
    (BinaryRequirement, resolve_binary_req),
    (PkgConfigRequirement, resolve_pkg_config_req),
    (PathRequirement, resolve_path_req),
    (CHeaderRequirement, resolve_c_header_req),
    (JavaScriptRuntimeRequirement, resolve_js_runtime_req),
    (ValaPackageRequirement, resolve_vala_package_req),
    (RubyGemRequirement, resolve_ruby_gem_req),
    (GoPackageRequirement, resolve_go_package_req),
    (DhAddonRequirement, resolve_dh_addon_req),
    (PhpClassRequirement, resolve_php_class_req),
    (RPackageRequirement, resolve_r_package_req),
    (NodePackageRequirement, resolve_node_package_req),
    (LibraryRequirement, resolve_library_req),
    (RubyFileRequirement, resolve_ruby_file_req),
    (XmlEntityRequirement, resolve_xml_entity_req),
    (SprocketsFileRequirement, resolve_sprockets_file_req),
    (JavaClassRequirement, resolve_java_class_req),
    (HaskellPackageRequirement, resolve_haskell_package_req),
    (MavenArtifactRequirement, resolve_maven_artifact_req),
    (GnomeCommonRequirement, resolve_gnome_common_req),
    (JDKFileRequirement, resolve_jdk_file_req),
    (PerlModuleRequirement, resolve_perl_module_req),
    (PerlFileRequirement, resolve_perl_file_req),
    (AutoconfMacroRequirement, resolve_autoconf_macro_req),
    (PythonModuleRequirement, resolve_python_module_req),
    (PythonPackageRequirement, resolve_python_package_req),
]


def resolve_requirement_apt(apt_mgr, req: UpstreamRequirement) -> AptRequirement:
    for rr_class, rr_fn in APT_REQUIREMENT_RESOLVERS:
        if isinstance(req, rr_class):
            return rr_fn(apt_mgr, req)
    raise NotImplementedError(type(req))


class AptResolver(Resolver):

    def __init__(self, apt):
        self.apt = apt

    def __str__(self):
        return "apt"

    @classmethod
    def from_session(cls, session):
        return cls(AptManager(session))

    def install(self, requirements):
        missing = []
        for req in requirements:
            try:
                if not req.met(self.apt.session):
                    missing.append(req)
            except NotImplementedError:
                missing.append(req)
        if not missing:
            return
        still_missing = []
        apt_requirements = []
        for m in missing:
            apt_req = self.resolve(m)
            if apt_req is None:
                still_missing.append(m)
            else:
                apt_requirements.append(m)
        self.apt.install(
            [req.package for req in apt_requirements])
        if still_missing:
            raise UnsatisfiedRequirements(still_missing)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)

    def resolve(self, req: UpstreamRequirement):
        return resolve_requirement_apt(self.apt, req)

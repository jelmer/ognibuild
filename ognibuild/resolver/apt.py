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

from itertools import chain
import logging
import os
import posixpath
import re
from typing import Optional, List, Tuple, Callable, Type

from debian.changelog import Version
from debian.deb822 import PkgRelation

from ..debian.apt import AptManager

from . import Resolver, UnsatisfiedRequirements
from .. import OneOfRequirement
from ..requirements import (
    Requirement,
    CargoCrateRequirement,
    BinaryRequirement,
    CHeaderRequirement,
    PkgConfigRequirement,
    PathRequirement,
    JavaScriptRuntimeRequirement,
    ValaPackageRequirement,
    RubyGemRequirement,
    GoPackageRequirement,
    GoRequirement,
    DhAddonRequirement,
    PhpClassRequirement,
    PhpPackageRequirement,
    RPackageRequirement,
    NodeModuleRequirement,
    NodePackageRequirement,
    LibraryRequirement,
    BoostComponentRequirement,
    StaticLibraryRequirement,
    RubyFileRequirement,
    XmlEntityRequirement,
    OctavePackageRequirement,
    SprocketsFileRequirement,
    JavaClassRequirement,
    CMakefileRequirement,
    HaskellPackageRequirement,
    MavenArtifactRequirement,
    GnomeCommonRequirement,
    JDKFileRequirement,
    JDKRequirement,
    JRERequirement,
    QTRequirement,
    X11Requirement,
    PerlModuleRequirement,
    PerlFileRequirement,
    AutoconfMacroRequirement,
    PythonModuleRequirement,
    PythonPackageRequirement,
    CertificateAuthorityRequirement,
    LibtoolRequirement,
    VagueDependencyRequirement,
    PerlPreDeclaredRequirement,
    IntrospectionTypelibRequirement,
    PHPExtensionRequirement,
)


class AptRequirement(Requirement):
    def __init__(self, relations):
        super(AptRequirement, self).__init__("apt")
        if not isinstance(relations, list):
            raise TypeError(relations)
        self.relations = relations

    @classmethod
    def simple(cls, package, minimum_version=None):
        rel = {"name": package}
        if minimum_version is not None:
            rel["version"] = (">=", minimum_version)
        return cls([[rel]])

    @classmethod
    def from_str(cls, text):
        return cls(PkgRelation.parse_relations(text))

    def pkg_relation_str(self):
        return PkgRelation.str(self.relations)

    def __hash__(self):
        return hash((type(self), self.pkg_relation_str()))

    def __eq__(self, other):
        return isinstance(self, type(other)) and self.relations == other.relations

    def __str__(self):
        return "apt requirement: %s" % self.pkg_relation_str()

    def __repr__(self):
        return "%s.from_str(%r)" % (type(self).__name__, self.pkg_relation_str())

    def package_names(self):
        for rel in self.relations:
            for entry in rel:
                yield entry["name"]

    def touches_package(self, package):
        for name in self.package_names():
            if name == package:
                return True
        return False

    def satisfied_by(self, binaries, version):
        def binary_pkg_matches(entry, binary):
            # TODO(jelmer): check versions
            if entry['name'] == binary['Package']:
                return True
            for provides_top in PkgRelation.parse_relations(
                    binary.get('Provides', '')):
                for provides in provides_top:
                    if entry['name'] == provides['name']:
                        return True
            return False

        for rel in self.relations:
            for entry in rel:
                if any(binary_pkg_matches(entry, binary) for binary in binaries):
                    break
            else:
                return False
        return True


def resolve_perl_predeclared_req(apt_mgr, req):
    try:
        req = req.lookup_module()
    except KeyError:
        logging.warning(
            'Unable to map predeclared function %s to a perl module', req.name)
        return None
    return resolve_perl_module_req(apt_mgr, req)


def find_package_names(
    apt_mgr: AptManager, paths: List[str], regex: bool = False, case_insensitive=False
) -> List[str]:
    if not isinstance(paths, list):
        raise TypeError(paths)
    return apt_mgr.get_packages_for_paths(paths, regex, case_insensitive)


def find_reqs_simple(
    apt_mgr: AptManager,
    paths: List[str],
    regex: bool = False,
    minimum_version=None,
    case_insensitive=False,
) -> List[str]:
    if not isinstance(paths, list):
        raise TypeError(paths)
    return [
        AptRequirement.simple(package, minimum_version=minimum_version)
        for package in find_package_names(apt_mgr, paths, regex, case_insensitive)
    ]


def python_spec_to_apt_rels(pkg_name, specs):
    # TODO(jelmer): Dealing with epoch, etc?
    if not specs:
        return [[{"name": pkg_name}]]
    else:
        rels = []
        for spec in specs:
            if spec[0] == "~=":
                # PEP 440: For a given release identifier V.N , the compatible
                # release clause is approximately equivalent to the pair of
                # comparison clauses: >= V.N, == V.*
                parts = spec[1].split(".")
                parts.pop(-1)
                parts[-1] = str(int(parts[-1]) + 1)
                next_maj_deb_version = Version(".".join(parts))
                deb_version = Version(spec[1])
                rels.extend(
                    [[{"name": pkg_name, "version": (">=", deb_version)}],
                     [{"name": pkg_name, "version": ("<<", next_maj_deb_version)}]])
            elif spec[0] == "!=":
                deb_version = Version(spec[1])
                rels.extend([
                    [{"name": pkg_name, "version": (">>", deb_version)}],
                    [{"name": pkg_name, "version": ("<<", deb_version)}]])
            elif spec[1].endswith(".*") and spec[0] == "==":
                s = spec[1].split(".")
                s.pop(-1)
                n = list(s)
                n[-1] = str(int(n[-1]) + 1)
                rels.extend(
                    [[{"name": pkg_name, "version": (">=", Version(".".join(s)))}],
                     [{"name": pkg_name, "version": ("<<", Version(".".join(n)))}]])
            else:
                c = {">=": ">=", "<=": "<=", "<": "<<", ">": ">>", "==": "="}[spec[0]]
                deb_version = Version(spec[1])
                rels.append([{"name": pkg_name, "version": (c, deb_version)}])
        return rels


def get_package_for_python_package(
    apt_mgr, package, python_version: Optional[str], specs=None
):
    pypy_regex = "/usr/lib/pypy/dist-packages/%s-.*\.(dist|egg)-info" % re.escape(
        package.replace("-", "_")
    )
    cpython2_regex = (
        "/usr/lib/python2\\.[0-9]/dist-packages/%s-.*\\.(dist|egg)-info"
        % re.escape(package.replace("-", "_"))
    )
    cpython3_regex = "/usr/lib/python3/dist-packages/%s-.*\\.(dist|egg)-info" % re.escape(
        package.replace("-", "_")
    )
    if python_version == "pypy":
        paths = [pypy_regex]
    elif python_version == "cpython2":
        paths = [cpython2_regex]
    elif python_version == "cpython3":
        paths = [cpython3_regex]
    elif python_version is None:
        paths = [cpython3_regex, cpython2_regex, pypy_regex]
    else:
        raise NotImplementedError("unsupported python version %s" % python_version)
    names = find_package_names(apt_mgr, paths, regex=True, case_insensitive=True)
    return [AptRequirement(python_spec_to_apt_rels(name, specs)) for name in names]


def get_package_for_python_module(apt_mgr, module, python_version, specs):
    cpython3_regexes = [
        posixpath.join(
            "/usr/lib/python3/dist-packages",
            re.escape(module.replace(".", "/")),
            "__init__.py",
        ),
        posixpath.join(
            "/usr/lib/python3/dist-packages",
            re.escape(module.replace(".", "/")) + ".py",
        ),
        posixpath.join(
            "/usr/lib/python3\\.[0-9]+/lib-dynload",
            re.escape(module.replace(".", "/")) + "\\.cpython-.*\\.so",
        ),
        posixpath.join(
            "/usr/lib/python3\\.[0-9]+/", re.escape(module.replace(".", "/")) + ".py"
        ),
        posixpath.join(
            "/usr/lib/python3\\.[0-9]+/",
            re.escape(module.replace(".", "/")),
            "__init__.py",
        ),
    ]
    cpython2_regexes = [
        posixpath.join(
            "/usr/lib/python2\\.[0-9]/dist-packages",
            re.escape(module.replace(".", "/")),
            "__init__.py",
        ),
        posixpath.join(
            "/usr/lib/python2\\.[0-9]/dist-packages",
            re.escape(module.replace(".", "/")) + ".py",
        ),
        posixpath.join(
            "/usr/lib/python2.\\.[0-9]/lib-dynload",
            re.escape(module.replace(".", "/")) + ".so",
        ),
    ]
    pypy_regexes = [
        posixpath.join(
            "/usr/lib/pypy/dist-packages",
            re.escape(module.replace(".", "/")),
            "__init__.py",
        ),
        posixpath.join(
            "/usr/lib/pypy/dist-packages", re.escape(module.replace(".", "/")) + ".py"
        ),
        posixpath.join(
            "/usr/lib/pypy/dist-packages",
            re.escape(module.replace(".", "/")) + "\\.pypy-.*\\.so",
        ),
    ]
    if python_version == "cpython3":
        paths = cpython3_regexes
    elif python_version == "cpython2":
        paths = cpython2_regexes
    elif python_version == "pypy":
        paths = pypy_regexes
    elif python_version is None:
        paths = cpython3_regexes + cpython2_regexes + pypy_regexes
    else:
        raise AssertionError("unknown python version %r" % python_version)
    names = find_package_names(apt_mgr, paths, regex=True)
    return [AptRequirement(python_spec_to_apt_rels(name, specs)) for name in names]


vague_map = {
    "the Gnu Scientific Library": "libgsl-dev",
    "the required FreeType library": "libfreetype-dev",
    "the Boost C++ libraries": "libboost-dev",
    "the sndfile library": "libsndfile-dev",

    # TODO(jelmer): Support resolving virtual packages
    "PythonLibs": "libpython3-dev",
    "PythonInterp": "python3",
    "ZLIB": "libz3-dev",
    "Osmium": "libosmium2-dev",
    "glib": "libglib2.0-dev",
    "OpenGL": "libgl-dev",

    # TODO(jelmer): For Python, check minimum_version and map to python 2 or python 3
    "Python": "libpython3-dev",
    "Lua": "liblua5.4-dev",
}


def resolve_vague_dep_req(apt_mgr, req):
    name = req.name
    options = []
    if ' or ' in name:
        for entry in name.split(' or '):
            options.extend(resolve_vague_dep_req(apt_mgr, VagueDependencyRequirement(entry)))

    if name in vague_map:
        options.append(AptRequirement.simple(vague_map[name], minimum_version=req.minimum_version))
    for x in req.expand():
        options.extend(resolve_requirement_apt(apt_mgr, x))

    if name.startswith('GNU '):
        options.extend(resolve_vague_dep_req(apt_mgr, VagueDependencyRequirement(name[4:])))

    if name.startswith('py') or name.endswith('py'):
        # TODO(jelmer): Try harder to determine whether this is a python package
        options.append(resolve_requirement_apt(apt_mgr, PythonPackageRequirement(name)))

    # Try even harder
    if not options:
        options.extend(find_reqs_simple(
            apt_mgr,
            [
                posixpath.join("/usr/lib", ".*", "pkgconfig", re.escape(req.name) + "-.*\\.pc"),
                posixpath.join("/usr/lib/pkgconfig", re.escape(req.name) + "-.*\\.pc")
            ],
            regex=True,
            case_insensitive=True,
            minimum_version=req.minimum_version
        ))

    return options


def resolve_php_extension_req(apt_mgr, req):
    return [AptRequirement.simple("php-%s" % req.extension)]


def resolve_octave_pkg_req(apt_mgr, req):
    return [AptRequirement.simple(
            "octave-%s" % req.package, minimum_version=req.minimum_version)]


def resolve_binary_req(apt_mgr, req):
    if posixpath.isabs(req.binary_name):
        paths = [req.binary_name]
    else:
        paths = [
            posixpath.join(dirname, req.binary_name) for dirname in ["/usr/bin", "/bin"]
        ]
    return find_reqs_simple(apt_mgr, paths)


def resolve_pkg_config_req(apt_mgr, req):
    names = find_package_names(
        apt_mgr,
        [
            posixpath.join(
                "/usr/lib", ".*", "pkgconfig", re.escape(req.module) + "\\.pc"
            )
        ],
        regex=True,
    )
    if not names:
        names = find_package_names(
            apt_mgr, [posixpath.join("/usr/lib/pkgconfig", req.module + ".pc")]
        )
    return [
        AptRequirement.simple(name, minimum_version=req.minimum_version)
        for name in names
    ]


def resolve_path_req(apt_mgr, req):
    return find_reqs_simple(apt_mgr, [req.path])


def resolve_c_header_req(apt_mgr, req):
    reqs = find_reqs_simple(
        apt_mgr, [posixpath.join("/usr/include", req.header)], regex=False
    )
    if not reqs:
        reqs = find_reqs_simple(
            apt_mgr,
            [posixpath.join("/usr/include", ".*", re.escape(req.header))],
            regex=True,
        )
    return reqs


def resolve_js_runtime_req(apt_mgr, req):
    return find_reqs_simple(apt_mgr, ["/usr/bin/node", "/usr/bin/duk"])


def resolve_vala_package_req(apt_mgr, req):
    path = "/usr/share/vala-[0-9.]+/vapi/%s\\.vapi" % re.escape(req.package)
    return find_reqs_simple(apt_mgr, [path], regex=True)


def resolve_ruby_gem_req(apt_mgr, req):
    paths = [
        posixpath.join(
            "/usr/share/rubygems-integration/all/"
            "specifications/%s-.*\\.gemspec" % re.escape(req.gem)
        )
    ]
    return find_reqs_simple(
        apt_mgr, paths, regex=True, minimum_version=req.minimum_version
    )


def resolve_go_package_req(apt_mgr, req):
    return find_reqs_simple(
        apt_mgr,
        [posixpath.join("/usr/share/gocode/src", re.escape(req.package), ".*")],
        regex=True,
    )


def resolve_go_req(apt_mgr, req):
    return [AptRequirement.simple("golang-go", minimum_version="2:%s~" % req.version)]


def resolve_dh_addon_req(apt_mgr, req):
    paths = [posixpath.join("/usr/share/perl5", req.path)]
    return find_reqs_simple(apt_mgr, paths)


def resolve_php_class_req(apt_mgr, req):
    path = "/usr/share/php/%s.php" % req.php_class.replace("\\", "/")
    return find_reqs_simple(apt_mgr, [path])


def resolve_php_package_req(apt_mgr, req):
    return [
        AptRequirement.simple("php-%s" % req.package, minimum_version=req.min_version)
    ]


def resolve_r_package_req(apt_mgr, req):
    paths = [
        posixpath.join("/usr/lib/R/site-library", req.package, "DESCRIPTION")
    ]
    return find_reqs_simple(apt_mgr, paths, minimum_version=req.minimum_version)


def resolve_node_module_req(apt_mgr, req):
    paths = [
        "/usr/share/nodejs/.*/node_modules/%s/index.js" % re.escape(req.module),
        "/usr/lib/nodejs/%s/index.js" % re.escape(req.module),
        "/usr/share/nodejs/%s/index.js" % re.escape(req.module),
    ]
    return find_reqs_simple(apt_mgr, paths, regex=True)


def resolve_node_package_req(apt_mgr, req):
    paths = [
        "/usr/share/nodejs/.*/node_modules/%s/package\\.json" % re.escape(req.package),
        "/usr/lib/nodejs/%s/package\\.json" % re.escape(req.package),
        "/usr/share/nodejs/%s/package\\.json" % re.escape(req.package),
    ]
    return find_reqs_simple(apt_mgr, paths, regex=True)


def resolve_library_req(apt_mgr, req):
    paths = [
        posixpath.join("/usr/lib/lib%s.so$" % re.escape(req.library)),
        posixpath.join("/usr/lib/.*/lib%s.so$" % re.escape(req.library)),
        posixpath.join("/usr/lib/lib%s.a$" % re.escape(req.library)),
        posixpath.join("/usr/lib/.*/lib%s.a$" % re.escape(req.library)),
    ]
    return find_reqs_simple(apt_mgr, paths, regex=True)


def resolve_static_library_req(apt_mgr, req):
    paths = [
        posixpath.join("/usr/lib/%s$" % re.escape(req.filename)),
        posixpath.join("/usr/lib/.*/%s$" % re.escape(req.filename)),
    ]
    return find_reqs_simple(apt_mgr, paths, regex=True)


def resolve_ruby_file_req(apt_mgr, req):
    paths = [posixpath.join("/usr/lib/ruby/vendor_ruby/%s.rb" % req.filename)]
    reqs = find_reqs_simple(apt_mgr, paths, regex=False)
    if reqs:
        return reqs
    paths = [
        posixpath.join(
            r"/usr/share/rubygems-integration/all/gems/([^/]+)/"
            "lib/%s\\.rb" % re.escape(req.filename)
        )
    ]
    return find_reqs_simple(apt_mgr, paths, regex=True)


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

    return find_reqs_simple(apt_mgr, [search_path], regex=False)


def resolve_sprockets_file_req(apt_mgr, req):
    if req.content_type == "application/javascript":
        path = "/usr/share/.*/app/assets/javascripts/%s\\.js$" % re.escape(req.name)
    else:
        logging.warning("unable to handle content type %s", req.content_type)
        return None
    return find_reqs_simple(apt_mgr, [path], regex=True)


def resolve_java_class_req(apt_mgr, req):
    apt_mgr.satisfy(["java-propose-classpath"])
    output = apt_mgr.session.check_output(
        ["java-propose-classpath", "-c" + req.classname]
    )
    classpath = [p for p in output.decode().strip(":").strip().split(":") if p]
    if not classpath:
        logging.warning("unable to find classpath for %s", req.classname)
        return False
    logging.info("Classpath for %s: %r", req.classname, classpath)
    return find_reqs_simple(apt_mgr, [classpath])


def resolve_cmake_file_req(apt_mgr, req):
    paths = ['/usr/lib/.*/cmake/.*/%s' % re.escape(req.filename)]
    return find_reqs_simple(apt_mgr, paths, regex=True)


def resolve_haskell_package_req(apt_mgr, req):
    path = "/var/lib/ghc/package\\.conf\\.d/%s-.*\\.conf" % re.escape(req.package)
    return find_reqs_simple(apt_mgr, [path], regex=True)


def resolve_maven_artifact_req(apt_mgr, req):
    if req.version is None:
        version = ".*"
        regex = True
        escape = re.escape
    else:
        version = req.version
        regex = False

        def escape(x):
            return x

    kind = req.kind or "jar"
    path = posixpath.join(
        escape("/usr/share/maven-repo"),
        escape(req.group_id.replace(".", "/")),
        escape(req.artifact_id),
        version,
        escape("%s-" % req.artifact_id) + version + escape("." + kind),
    )

    return find_reqs_simple(apt_mgr, [path], regex=regex)


def resolve_gnome_common_req(apt_mgr, req):
    return [AptRequirement.simple("gnome-common")]


def resolve_jdk_file_req(apt_mgr, req):
    path = re.escape(req.jdk_path) + ".*/" + re.escape(req.filename)
    return find_reqs_simple(apt_mgr, [path], regex=True)


def resolve_jdk_req(apt_mgr, req):
    return [AptRequirement.simple("default-jdk")]


def resolve_jre_req(apt_mgr, req):
    return [AptRequirement.simple("default-jre")]


def resolve_x11_req(apt_mgr, req):
    return [AptRequirement.simple("libx11-dev")]


def resolve_qt_req(apt_mgr, req):
    return find_reqs_simple(apt_mgr, ["/usr/lib/.*/qt[0-9]+/bin/qmake"], regex=True)


def resolve_libtool_req(apt_mgr, req):
    return [AptRequirement.simple("libtool")]


def resolve_perl_module_req(apt_mgr, req):
    DEFAULT_PERL_PATHS = ["/usr/share/perl5", "/usr/lib/.*/perl5/.*", "/usr/lib/.*/perl-base", "/usr/lib/.*/perl/[^/]+", "/usr/share/perl/[^/]+"]

    if req.inc is None:
        if req.filename is None:
            paths = [posixpath.join(inc, re.escape(req.module.replace('::', '/') + '.pm')) for inc in DEFAULT_PERL_PATHS]
            regex = True
        elif not posixpath.isabs(req.filename):
            paths = [posixpath.join(inc, re.escape(req.filename)) for inc in DEFAULT_PERL_PATHS]
            regex = True
        else:
            paths = [req.filename]
            regex = False
    else:
        regex = False
        paths = [posixpath.join(inc, req.filename) for inc in req.inc]
    return find_reqs_simple(apt_mgr, paths, regex=regex)


def resolve_perl_file_req(apt_mgr, req):
    return find_reqs_simple(apt_mgr, [req.filename], regex=False)


def _m4_macro_regex(macro):
    defun_prefix = re.escape("AC_DEFUN([%s]," % macro)
    au_alias_prefix = re.escape("AU_ALIAS([%s]," % macro)
    m4_copy = r"m4_copy\(.*,\s*\[%s\]\)" % re.escape(macro)
    return "(" + "|".join([defun_prefix, au_alias_prefix, m4_copy]) + ")"


def _find_local_m4_macro(macro):
    # TODO(jelmer): Query some external service that can search all binary packages?
    p = re.compile(_m4_macro_regex(macro).encode('ascii'))
    for entry in os.scandir("/usr/share/aclocal"):
        if not entry.is_file():
            continue
        with open(entry.path, "rb") as f:
            for line in f:
                if any(p.finditer(line)):
                    return entry.path
    raise KeyError


def resolve_autoconf_macro_req(apt_mgr, req):
    try:
        path = _find_local_m4_macro(req.macro)
    except KeyError:
        logging.info("No local m4 file found defining %s", req.macro)
        return None
    return find_reqs_simple(apt_mgr, [path])


def resolve_python_module_req(apt_mgr, req):
    if req.minimum_version:
        specs = [(">=", req.minimum_version)]
    else:
        specs = []
    if req.python_version == 2:
        return get_package_for_python_module(apt_mgr, req.module, "cpython2", specs)
    elif req.python_version in (None, 3):
        return get_package_for_python_module(apt_mgr, req.module, "cpython3", specs)
    else:
        return None


def resolve_python_package_req(apt_mgr, req):
    if req.python_version == 2:
        return get_package_for_python_package(
            apt_mgr, req.package, "cpython2", req.specs
        )
    elif req.python_version in (None, 3):
        return get_package_for_python_package(
            apt_mgr, req.package, "cpython3", req.specs
        )
    else:
        return None


def resolve_cargo_crate_req(apt_mgr, req):
    paths = ["/usr/share/cargo/registry/%s-[0-9]+.*/Cargo.toml" % re.escape(req.crate)]
    return find_reqs_simple(apt_mgr, paths, regex=True)


def resolve_ca_req(apt_mgr, req):
    return [AptRequirement.simple("ca-certificates")]


def resolve_introspection_typelib_req(apt_mgr, req):
    return find_reqs_simple(
        apt_mgr, [r'/usr/lib/.*/girepository-.*/%s-.*\.typelib' % re.escape(req.library)],
        regex=True)


def resolve_apt_req(apt_mgr, req):
    # TODO(jelmer): This should be checking whether versions match as well.
    for package_name in req.package_names():
        if not apt_mgr.package_exists(package_name):
            return []
    return [req]


def resolve_boost_component_req(apt_mgr, req):
    return find_reqs_simple(
        apt_mgr, ["/usr/lib/.*/libboost_%s" % re.escape(req.name)],
        regex=True)


def resolve_oneof_req(apt_mgr, req):
    options = [resolve_requirement_apt(apt_mgr, req) for req in req.elements]
    ret = []
    for option in options:
        if not option:
            return False
        ret.append(option[0])
    return ret


APT_REQUIREMENT_RESOLVERS: List[Tuple[Type[Requirement], Callable[[AptManager, Requirement], List[AptRequirement]]]] = [
    (AptRequirement, resolve_apt_req),
    (BinaryRequirement, resolve_binary_req),
    (VagueDependencyRequirement, resolve_vague_dep_req),
    (PerlPreDeclaredRequirement, resolve_perl_predeclared_req),
    (PkgConfigRequirement, resolve_pkg_config_req),
    (PathRequirement, resolve_path_req),
    (CHeaderRequirement, resolve_c_header_req),
    (JavaScriptRuntimeRequirement, resolve_js_runtime_req),
    (ValaPackageRequirement, resolve_vala_package_req),
    (RubyGemRequirement, resolve_ruby_gem_req),
    (GoPackageRequirement, resolve_go_package_req),
    (GoRequirement, resolve_go_req),
    (DhAddonRequirement, resolve_dh_addon_req),
    (PhpClassRequirement, resolve_php_class_req),
    (PhpPackageRequirement, resolve_php_package_req),
    (RPackageRequirement, resolve_r_package_req),
    (NodeModuleRequirement, resolve_node_module_req),
    (NodePackageRequirement, resolve_node_package_req),
    (LibraryRequirement, resolve_library_req),
    (StaticLibraryRequirement, resolve_static_library_req),
    (RubyFileRequirement, resolve_ruby_file_req),
    (XmlEntityRequirement, resolve_xml_entity_req),
    (SprocketsFileRequirement, resolve_sprockets_file_req),
    (JavaClassRequirement, resolve_java_class_req),
    (CMakefileRequirement, resolve_cmake_file_req),
    (HaskellPackageRequirement, resolve_haskell_package_req),
    (MavenArtifactRequirement, resolve_maven_artifact_req),
    (GnomeCommonRequirement, resolve_gnome_common_req),
    (JDKFileRequirement, resolve_jdk_file_req),
    (JDKRequirement, resolve_jdk_req),
    (JRERequirement, resolve_jre_req),
    (QTRequirement, resolve_qt_req),
    (X11Requirement, resolve_x11_req),
    (LibtoolRequirement, resolve_libtool_req),
    (PerlModuleRequirement, resolve_perl_module_req),
    (PerlFileRequirement, resolve_perl_file_req),
    (AutoconfMacroRequirement, resolve_autoconf_macro_req),
    (PythonModuleRequirement, resolve_python_module_req),
    (PythonPackageRequirement, resolve_python_package_req),
    (CertificateAuthorityRequirement, resolve_ca_req),
    (CargoCrateRequirement, resolve_cargo_crate_req),
    (IntrospectionTypelibRequirement, resolve_introspection_typelib_req),
    (BoostComponentRequirement, resolve_boost_component_req),
    (PHPExtensionRequirement, resolve_php_extension_req),
    (OctavePackageRequirement, resolve_octave_pkg_req),
    (OneOfRequirement, resolve_oneof_req),
]


def resolve_requirement_apt(apt_mgr, req: Requirement) -> List[AptRequirement]:
    for rr_class, rr_fn in APT_REQUIREMENT_RESOLVERS:
        if isinstance(req, rr_class):
            ret = rr_fn(apt_mgr, req)
            if not ret:
                return []
            if not isinstance(ret, list):
                raise TypeError(ret)
            return ret
    raise NotImplementedError(type(req))


def default_tie_breakers(session):
    from ..debian.udd import popcon_tie_breaker
    from ..debian.build_deps import BuildDependencyTieBreaker
    return [
        BuildDependencyTieBreaker.from_session(session),
        popcon_tie_breaker,
        ]


class AptResolver(Resolver):
    def __init__(self, apt, tie_breakers=None):
        self.apt = apt
        if tie_breakers is None:
            tie_breakers = default_tie_breakers(apt.session)
        self.tie_breakers = tie_breakers

    def __str__(self):
        return "apt"

    def __repr__(self):
        return "%s(%r, %r)" % (type(self).__name__, self.apt, self.tie_breakers)

    @classmethod
    def from_session(cls, session, tie_breakers=None):
        return cls(AptManager.from_session(session), tie_breakers=tie_breakers)

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
                apt_requirements.append(apt_req)
        if apt_requirements:
            self.apt.satisfy(
                [PkgRelation.str(chain(*[r.relations for r in apt_requirements]))]
            )
        if still_missing:
            raise UnsatisfiedRequirements(still_missing)

    def explain(self, requirements):
        apt_requirements = []
        for r in requirements:
            apt_req = self.resolve(r)
            if apt_req is not None:
                apt_requirements.append((r, apt_req))
        if apt_requirements:
            yield (
                self.apt.satisfy_command(
                    [
                        PkgRelation.str(
                            chain(*[r.relations for o, r in apt_requirements])
                        )
                    ]
                ),
                [o for o, r in apt_requirements],
            )

    def resolve(self, req: Requirement):
        ret = resolve_requirement_apt(self.apt, req)
        if not ret:
            return None
        if len(ret) == 1:
            return ret[0]
        logging.info("Need to break tie between %r with %r", ret, self.tie_breakers)
        for tie_breaker in self.tie_breakers:
            winner = tie_breaker(ret)
            if winner is not None:
                if not isinstance(winner, AptRequirement):
                    raise TypeError(winner)
                return winner
        logging.info("Unable to break tie over %r, picking first: %r", ret, ret[0])
        return ret[0]

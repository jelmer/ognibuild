#!/usr/bin/python
# Copyright (C) 2018 Jelmer Vernooij <jelmer@jelmer.uk>
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

__all__ = [
    'build_incrementally',
]

import logging
import os
import re
import subprocess
import sys
from typing import Iterator, List, Callable, Type, Tuple, Set

from debian.deb822 import (
    Deb822,
    PkgRelation,
    Release,
    )

from breezy.commit import PointlessCommit
from breezy.tree import Tree
from debmutate.control import (
    ensure_some_version,
    ensure_minimum_version,
    pg_buildext_updatecontrol,
    ControlEditor,
    )
from debmutate.debhelper import (
    get_debhelper_compat_level,
    )
from debmutate.deb822 import (
    Deb822Editor,
    )
from debmutate.reformatting import (
    FormattingUnpreservable,
    GeneratedFile,
    )
from lintian_brush import (
    reset_tree,
    )
from lintian_brush.changelog import (
    add_changelog_entry,
    )

from lintian_brush.rules import (
    dh_invoke_add_with,
    update_rules,
    )
from silver_platter.debian import (
    debcommit,
    DEFAULT_BUILDER,
    )

from breezy.plugins.debian.util import get_build_architecture
from .build import attempt_build
from buildlog_consultant.sbuild import (
    Problem,
    MissingConfigStatusInput,
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
    MissingConfigure,
    MissingAutomakeInput,
    MissingRPackage,
    MissingRubyFile,
    MissingAutoconfMacro,
    MissingValaPackage,
    MissingXfceDependency,
    MissingHaskellDependencies,
    NeedPgBuildExtUpdateControl,
    SbuildFailure,
    DhAddonLoadFailure,
    AptFetchFailure,
    MissingMavenArtifacts,
    GnomeCommonMissing,
    MissingGnomeCommonDependency,
    )


DEFAULT_MAX_ITERATIONS = 10


class CircularDependency(Exception):
    """Adding dependency would introduce cycle."""

    def __init__(self, package):
        self.package = package


class DependencyContext(object):

    def __init__(self, tree, subpath='', committer=None,
                 update_changelog=True):
        self.tree = tree
        self.subpath = subpath
        self.committer = committer
        self.update_changelog = update_changelog

    def add_dependency(self, package, minimum_version=None):
        raise NotImplementedError(self.add_dependency)


class BuildDependencyContext(DependencyContext):

    def add_dependency(self, package, minimum_version=None):
        return add_build_dependency(
            self.tree, package, minimum_version=minimum_version,
            committer=self.committer, subpath=self.subpath,
            update_changelog=self.update_changelog)


class AutopkgtestDependencyContext(DependencyContext):

    def __init__(self, testname, tree, subpath='', committer=None,
                 update_changelog=True):
        self.testname = testname
        super(AutopkgtestDependencyContext, self).__init__(
            tree, subpath, committer, update_changelog)

    def add_dependency(self, package, minimum_version=None):
        return add_test_dependency(
            self.tree, self.testname, package,
            minimum_version=minimum_version,
            committer=self.committer, subpath=self.subpath,
            update_changelog=self.update_changelog)


def add_build_dependency(tree, package, minimum_version=None,
                         committer=None, subpath='', update_changelog=True):
    if not isinstance(package, str):
        raise TypeError(package)

    control_path = os.path.join(tree.abspath(subpath), 'debian/control')
    try:
        with ControlEditor(path=control_path) as updater:
            for binary in updater.binaries:
                if binary["Package"] == package:
                    raise CircularDependency(package)
            if minimum_version:
                updater.source["Build-Depends"] = ensure_minimum_version(
                    updater.source.get("Build-Depends", ""),
                    package, minimum_version)
            else:
                updater.source["Build-Depends"] = ensure_some_version(
                    updater.source.get("Build-Depends", ""), package)
    except FormattingUnpreservable as e:
        logging.info(
            'Unable to edit %s in a way that preserves formatting.',
            e.path)
        return False

    if minimum_version:
        desc = "%s (>= %s)" % (package, minimum_version)
    else:
        desc = package

    if not updater.changed:
        logging.info('Giving up; dependency %s was already present.', desc)
        return False

    logging.info("Adding build dependency: %s", desc)
    return commit_debian_changes(
        tree, subpath, "Add missing build dependency on %s." % desc,
        committer=committer, update_changelog=update_changelog)


def add_test_dependency(tree, testname, package, minimum_version=None,
                        committer=None, subpath='', update_changelog=True):
    if not isinstance(package, str):
        raise TypeError(package)

    tests_control_path = os.path.join(
        tree.abspath(subpath), 'debian/tests/control')

    try:
        with Deb822Editor(path=tests_control_path) as updater:
            command_counter = 1
            for control in updater.paragraphs:
                try:
                    name = control["Tests"]
                except KeyError:
                    name = "command%d" % command_counter
                    command_counter += 1
                if name != testname:
                    continue
                if minimum_version:
                    control["Depends"] = ensure_minimum_version(
                        control.get("Depends", ""),
                        package, minimum_version)
                else:
                    control["Depends"] = ensure_some_version(
                        control.get("Depends", ""), package)
    except FormattingUnpreservable as e:
        logging.info(
            'Unable to edit %s in a way that preserves formatting.',
            e.path)
        return False
    if not updater.changed:
        return False

    if minimum_version:
        desc = "%s (>= %s)" % (package, minimum_version)
    else:
        desc = package

    logging.info("Adding dependency to test %s: %s", testname, desc)
    return commit_debian_changes(
        tree, subpath,
        "Add missing dependency for test %s on %s." % (testname, desc),
        update_changelog=update_changelog)


def commit_debian_changes(tree, subpath, summary, committer=None,
                          update_changelog=True):
    with tree.lock_write():
        try:
            if update_changelog:
                add_changelog_entry(
                    tree, os.path.join(subpath, 'debian/changelog'), [summary])
                debcommit(tree, committer=committer, subpath=subpath)
            else:
                tree.commit(message=summary, committer=committer,
                            specific_files=[subpath])
        except PointlessCommit:
            return False
        else:
            return True


class FileSearcher(object):

    def search_files(self, path, regex=False):
        raise NotImplementedError(self.search_files)


class ContentsFileNotFound(Exception):
    """The contents file was not found."""


class AptContentsFileSearcher(FileSearcher):

    def __init__(self):
        self._db = {}

    @classmethod
    def from_env(cls):
        sources = os.environ['REPOSITORIES'].split(':')
        return cls.from_repositories(sources)

    def __setitem__(self, path, package):
        self._db[path] = package

    def search_files(self, path, regex=False):
        for p, pkg in sorted(self._db.items()):
            if regex:
                if re.match(path, p):
                    yield pkg
            else:
                if path == p:
                    yield pkg

    def load_file(self, f):
        for line in f:
            (path, rest) = line.rsplit(maxsplit=1)
            package = rest.split(b'/')[-1]
            decoded_path = '/' + path.decode('utf-8', 'surrogateescape')
            self[decoded_path] = package.decode('utf-8')

    @classmethod
    def from_urls(cls, urls):
        self = cls()
        for url in urls:
            self.load_url(url)
        return self

    @classmethod
    def from_repositories(cls, sources):
        # TODO(jelmer): Verify signatures, etc.
        urls = []
        arches = [get_build_architecture(), 'all']
        for source in sources:
            parts = source.split(' ')
            if parts[0] != 'deb':
                logging.warning('Invalid line in sources: %r', source)
                continue
            base_url = parts[1]
            name = parts[2]
            components = parts[3:]
            response = cls._get('%s/%s/Release' % (base_url, name))
            r = Release(response)
            desired_files = set()
            for component in components:
                for arch in arches:
                    desired_files.add('%s/Contents-%s' % (component, arch))
            for entry in r['MD5Sum']:
                if entry['name'] in desired_files:
                    urls.append('%s/%s/%s' % (base_url, name, entry['name']))
        return cls.from_urls(urls)

    @staticmethod
    def _get(url):
        from urllib.request import urlopen, Request
        request = Request(url, headers={'User-Agent': 'Debian Janitor'})
        return urlopen(request)

    def load_url(self, url):
        from urllib.error import HTTPError
        try:
            response = self._get(url)
        except HTTPError as e:
            if e.status == 404:
                raise ContentsFileNotFound(url)
            raise
        if url.endswith('.gz'):
            import gzip
            f = gzip.GzipFile(fileobj=response)
        elif response.headers.get_content_type() == 'text/plain':
            f = response
        else:
            raise Exception(
                'Unknown content type %r' %
                response.headers.get_content_type())
        self.load_file(f)


class GeneratedFileSearcher(FileSearcher):

    def __init__(self, db):
        self._db = db

    def search_files(self, path, regex=False):
        for p, pkg in sorted(self._db.items()):
            if regex:
                if re.match(path, p):
                    yield pkg
            else:
                if path == p:
                    yield pkg


# TODO(jelmer): read from a file
GENERATED_FILE_SEARCHER = GeneratedFileSearcher({
    '/etc/locale.gen': 'locales',
    # Alternative
    '/usr/bin/rst2html': '/usr/share/docutils/scripts/python3/rst2html'})


_apt_file_searcher = None


def search_apt_file(path: str, regex: bool = False) -> Iterator[FileSearcher]:
    global _apt_file_searcher
    if _apt_file_searcher is None:
        # TODO(jelmer): cache file
        _apt_file_searcher = AptContentsFileSearcher.from_env()
    if _apt_file_searcher:
        yield from _apt_file_searcher.search_files(path, regex=regex)
    yield from GENERATED_FILE_SEARCHER.search_files(path, regex=regex)


def get_package_for_paths(paths, regex=False):
    candidates = set()
    for path in paths:
        candidates.update(search_apt_file(path, regex=regex))
        if candidates:
            break
    if len(candidates) == 0:
        logging.warning('No packages found that contain %r', paths)
        return None
    if len(candidates) > 1:
        logging.warning(
            'More than 1 packages found that contain %r: %r',
            path, candidates)
        # Euhr. Pick the one with the shortest name?
        return sorted(candidates, key=len)[0]
    else:
        return candidates.pop()


def get_package_for_python_module(module, python_version):
    if python_version == 'python3':
        paths = [
            os.path.join(
                '/usr/lib/python3/dist-packages',
                module.replace('.', '/'),
                '__init__.py'),
            os.path.join(
                '/usr/lib/python3/dist-packages',
                module.replace('.', '/') + '.py'),
            os.path.join(
                '/usr/lib/python3\\.[0-9]+/lib-dynload',
                module.replace('.', '/') + '\\.cpython-.*\\.so'),
            os.path.join(
                '/usr/lib/python3\\.[0-9]+/',
                module.replace('.', '/') + '.py'),
            os.path.join(
                '/usr/lib/python3\\.[0-9]+/',
                module.replace('.', '/'), '__init__.py'),
            ]
    elif python_version == 'python2':
        paths = [
            os.path.join(
                '/usr/lib/python2\\.[0-9]/dist-packages',
                module.replace('.', '/'),
                '__init__.py'),
            os.path.join(
                '/usr/lib/python2\\.[0-9]/dist-packages',
                module.replace('.', '/') + '.py'),
            os.path.join(
                '/usr/lib/python2.\\.[0-9]/lib-dynload',
                module.replace('.', '/') + '.so')]
    elif python_version == 'pypy':
        paths = [
            os.path.join(
                '/usr/lib/pypy/dist-packages',
                module.replace('.', '/'),
                '__init__.py'),
            os.path.join(
                '/usr/lib/pypy/dist-packages',
                module.replace('.', '/') + '.py'),
            os.path.join(
                '/usr/lib/pypy/dist-packages',
                module.replace('.', '/') + '\\.pypy-.*\\.so'),
            ]
    else:
        raise AssertionError(
            'unknown python version %r' % python_version)
    return get_package_for_paths(paths, regex=True)


def targeted_python_versions(tree: Tree) -> Set[str]:
    with tree.get_file('debian/control') as f:
        control = Deb822(f)
    build_depends = PkgRelation.parse_relations(
        control.get('Build-Depends', ''))
    all_build_deps: Set[str] = set()
    for or_deps in build_depends:
        all_build_deps.update(or_dep['name'] for or_dep in or_deps)
    targeted = set()
    if any(x.startswith('pypy') for x in all_build_deps):
        targeted.add('pypy')
    if any(x.startswith('python-') for x in all_build_deps):
        targeted.add('cpython2')
    if any(x.startswith('python3-') for x in all_build_deps):
        targeted.add('cpython3')
    return targeted


apt_cache = None


def package_exists(package):
    global apt_cache
    if apt_cache is None:
        import apt_pkg
        apt_cache = apt_pkg.Cache()
    for p in apt_cache.packages:
        if p.name == package:
            return True
    return False


def fix_missing_javascript_runtime(error, context):
    package = get_package_for_paths(
        ['/usr/bin/node', '/usr/bin/duk'],
        regex=False)
    if package is None:
        return False
    return context.add_dependency(package)


def fix_missing_python_distribution(error, context):
    targeted = targeted_python_versions(context.tree)
    default = not targeted

    pypy_pkg = get_package_for_paths(
        ['/usr/lib/pypy/dist-packages/%s-.*.egg-info' % error.distribution],
        regex=True)
    if pypy_pkg is None:
        pypy_pkg = 'pypy-%s' % error.distribution
        if not package_exists(pypy_pkg):
            pypy_pkg = None

    py2_pkg = get_package_for_paths(
        ['/usr/lib/python2\\.[0-9]/dist-packages/%s-.*.egg-info' %
         error.distribution], regex=True)
    if py2_pkg is None:
        py2_pkg = 'python-%s' % error.distribution
        if not package_exists(py2_pkg):
            py2_pkg = None

    py3_pkg = get_package_for_paths(
        ['/usr/lib/python3/dist-packages/%s-.*.egg-info' %
         error.distribution], regex=True)
    if py3_pkg is None:
        py3_pkg = 'python3-%s' % error.distribution
        if not package_exists(py3_pkg):
            py3_pkg = None

    extra_build_deps = []
    if error.python_version == 2:
        if 'pypy' in targeted:
            if not pypy_pkg:
                logging.warning('no pypy package found for %s', error.module)
            else:
                extra_build_deps.append(pypy_pkg)
        if 'cpython2' in targeted or default:
            if not py2_pkg:
                logging.warning(
                    'no python 2 package found for %s', error.module)
                return False
            extra_build_deps.append(py2_pkg)
    elif error.python_version == 3:
        if not py3_pkg:
            logging.warning('no python 3 package found for %s', error.module)
            return False
        extra_build_deps.append(py3_pkg)
    else:
        if py3_pkg and ('cpython3' in targeted or default):
            extra_build_deps.append(py3_pkg)
        if py2_pkg and ('cpython2' in targeted or default):
            extra_build_deps.append(py2_pkg)
        if pypy_pkg and 'pypy' in targeted:
            extra_build_deps.append(pypy_pkg)

    if not extra_build_deps:
        return False

    for dep_pkg in extra_build_deps:
        assert dep_pkg is not None
        if not context.add_dependency(
                dep_pkg, minimum_version=error.minimum_version):
            return False
    return True


def fix_missing_python_module(error, context):
    if getattr(context, 'tree', None) is not None:
        targeted = targeted_python_versions(context.tree)
    else:
        targeted = set()
    default = (not targeted)

    pypy_pkg = get_package_for_python_module(error.module, 'pypy')
    py2_pkg = get_package_for_python_module(error.module, 'python2')
    py3_pkg = get_package_for_python_module(error.module, 'python3')

    extra_build_deps = []
    if error.python_version == 2:
        if 'pypy' in targeted:
            if not pypy_pkg:
                logging.warning('no pypy package found for %s', error.module)
            else:
                extra_build_deps.append(pypy_pkg)
        if 'cpython2' in targeted or default:
            if not py2_pkg:
                logging.warning(
                    'no python 2 package found for %s', error.module)
                return False
            extra_build_deps.append(py2_pkg)
    elif error.python_version == 3:
        if not py3_pkg:
            logging.warning(
                'no python 3 package found for %s', error.module)
            return False
        extra_build_deps.append(py3_pkg)
    else:
        if py3_pkg and ('cpython3' in targeted or default):
            extra_build_deps.append(py3_pkg)
        if py2_pkg and ('cpython2' in targeted or default):
            extra_build_deps.append(py2_pkg)
        if pypy_pkg and 'pypy' in targeted:
            extra_build_deps.append(pypy_pkg)

    if not extra_build_deps:
        return False

    for dep_pkg in extra_build_deps:
        assert dep_pkg is not None
        if not context.add_dependency(dep_pkg, error.minimum_version):
            return False
    return True


def fix_missing_go_package(error, context):
    package = get_package_for_paths(
        [os.path.join('/usr/share/gocode/src', error.package, '.*')],
        regex=True)
    if package is None:
        return False
    return context.add_dependency(package)


def fix_missing_c_header(error, context):
    package = get_package_for_paths(
        [os.path.join('/usr/include', error.header)], regex=False)
    if package is None:
        package = get_package_for_paths(
            [os.path.join('/usr/include', '.*', error.header)], regex=True)
    if package is None:
        return False
    return context.add_dependency(package)


def fix_missing_pkg_config(error, context):
    package = get_package_for_paths(
        [os.path.join('/usr/lib/pkgconfig', error.module + '.pc')])
    if package is None:
        package = get_package_for_paths(
            [os.path.join('/usr/lib', '.*', 'pkgconfig',
                          error.module + '.pc')],
            regex=True)
    if package is None:
        return False
    return context.add_dependency(
        package, minimum_version=error.minimum_version)


def fix_missing_command(error, context):
    if os.path.isabs(error.command):
        paths = [error.command]
    else:
        paths = [
            os.path.join(dirname, error.command)
            for dirname in ['/usr/bin', '/bin']]
    package = get_package_for_paths(paths)
    if package is None:
        logging.info('No packages found that contain %r', paths)
        return False
    return context.add_dependency(package)


def fix_missing_file(error, context):
    package = get_package_for_paths([error.path])
    if package is None:
        return False
    return context.add_dependency(package)


def fix_missing_sprockets_file(error, context):
    if error.content_type == 'application/javascript':
        path = '/usr/share/.*/app/assets/javascripts/%s.js$' % error.name
    else:
        logging.warning('unable to handle content type %s', error.content_type)
        return False
    package = get_package_for_paths([path], regex=True)
    if package is None:
        return False
    return context.add_dependency(package)


DEFAULT_PERL_PATHS = ['/usr/share/perl5']


def fix_missing_perl_file(error, context):

    if (error.filename == 'Makefile.PL' and
            not context.tree.has_filename('Makefile.PL') and
            context.tree.has_filename('dist.ini')):
        # TODO(jelmer): add dist-zilla add-on to debhelper
        raise NotImplementedError

    if error.inc is None:
        if error.filename is None:
            filename = error.module.replace('::', '/') + '.pm'
            paths = [os.path.join(inc, filename)
                     for inc in DEFAULT_PERL_PATHS]
        elif not os.path.isabs(error.filename):
            return False
        else:
            paths = [error.filename]
    else:
        paths = [os.path.join(inc, error.filename) for inc in error.inc]
    package = get_package_for_paths(paths, regex=False)
    if package is None:
        if getattr(error, 'module', None):
            logging.warning(
                'no perl package found for %s (%r).',
                error.module, error.filename)
        else:
            logging.warning(
                'perl file %s not found (paths searched for: %r).',
                error.filename, paths)
        return False
    return context.add_dependency(package)


def get_package_for_node_package(node_package):
    paths = [
        '/usr/share/nodejs/.*/node_modules/%s/package.json' % node_package,
        '/usr/lib/nodejs/%s/package.json' % node_package,
        '/usr/share/nodejs/%s/package.json' % node_package]
    return get_package_for_paths(paths, regex=True)


def fix_missing_node_module(error, context):
    package = get_package_for_node_package(error.module)
    if package is None:
        logging.warning(
            'no node package found for %s.',
            error.module)
        return False
    return context.add_dependency(package)


def fix_missing_dh_addon(error, context):
    paths = [os.path.join('/usr/share/perl5', error.path)]
    package = get_package_for_paths(paths)
    if package is None:
        logging.warning('no package for debhelper addon %s', error.name)
        return False
    return context.add_dependency(package)


def retry_apt_failure(error, context):
    return True


def fix_missing_php_class(error, context):
    path = '/usr/share/php/%s.php' % error.php_class.replace('\\', '/')
    package = get_package_for_paths([path])
    if package is None:
        logging.warning('no package for PHP class %s', error.php_class)
        return False
    return context.add_dependency(package)


def fix_missing_jdk_file(error, context):
    path = error.jdk_path + '.*/' + error.filename
    package = get_package_for_paths([path], regex=True)
    if package is None:
        logging.warning(
            'no package found for %s (JDK: %s) - regex %s',
            error.filename, error.jdk_path, path)
        return False
    return context.add_dependency(package)


def fix_missing_vala_package(error, context):
    path = '/usr/share/vala-[0-9.]+/vapi/%s.vapi' % error.package
    package = get_package_for_paths([path], regex=True)
    if package is None:
        logging.warning(
            'no file found for package %s - regex %s',
            error.package, path)
        return False
    return context.add_dependency(package)


def fix_missing_xml_entity(error, context):
    # Ideally we should be using the XML catalog for this, but hardcoding
    # a few URLs will do for now..
    URL_MAP = {
        'http://www.oasis-open.org/docbook/xml/':
            '/usr/share/xml/docbook/schema/dtd/'
    }
    for url, path in URL_MAP.items():
        if error.url.startswith(url):
            search_path = os.path.join(path, error.url[len(url):])
            break
    else:
        return False

    package = get_package_for_paths([search_path], regex=False)
    if package is None:
        return False
    return context.add_dependency(package)


def fix_missing_library(error, context):
    paths = [os.path.join('/usr/lib/lib%s.so$' % error.library),
             os.path.join('/usr/lib/.*/lib%s.so$' % error.library),
             os.path.join('/usr/lib/lib%s.a$' % error.library),
             os.path.join('/usr/lib/.*/lib%s.a$' % error.library)]
    package = get_package_for_paths(paths, regex=True)
    if package is None:
        logging.warning('no package for library %s', error.library)
        return False
    return context.add_dependency(package)


def fix_missing_ruby_gem(error, context):
    paths = [os.path.join(
        '/usr/share/rubygems-integration/all/'
        'specifications/%s-.*\\.gemspec' % error.gem)]
    package = get_package_for_paths(paths, regex=True)
    if package is None:
        logging.warning('no package for gem %s', error.gem)
        return False
    return context.add_dependency(package, minimum_version=error.version)


def fix_missing_ruby_file(error, context):
    paths = [
        os.path.join('/usr/lib/ruby/vendor_ruby/%s.rb' % error.filename)]
    package = get_package_for_paths(paths)
    if package is not None:
        return context.add_dependency(package)
    paths = [
        os.path.join(r'/usr/share/rubygems-integration/all/gems/([^/]+)/'
                     'lib/%s.rb' % error.filename)]
    package = get_package_for_paths(paths, regex=True)
    if package is not None:
        return context.add_dependency(package)

    logging.warning('no package for ruby file %s', error.filename)
    return False


def fix_missing_r_package(error, context):
    paths = [os.path.join('/usr/lib/R/site-library/.*/R/%s$' % error.package)]
    package = get_package_for_paths(paths, regex=True)
    if package is None:
        logging.warning('no package for R package %s', error.package)
        return False
    return context.add_dependency(
        package, minimum_version=error.minimum_version)


def fix_missing_java_class(error, context):
    # Unfortunately this only finds classes in jars installed on the host
    # system :(
    output = subprocess.check_output(
        ["java-propose-classpath", "-c" + error.classname])
    classpath = [
        p for p in output.decode().strip(":").strip().split(':') if p]
    if not classpath:
        logging.warning('unable to find classpath for %s', error.classname)
        return False
    logging.info('Classpath for %s: %r', error.classname, classpath)
    package = get_package_for_paths(classpath)
    if package is None:
        logging.warning('no package for files in %r', classpath)
        return False
    return context.add_dependency(package)


def enable_dh_autoreconf(context):
    # Debhelper >= 10 depends on dh-autoreconf and enables autoreconf by
    # default.
    debhelper_compat_version = get_debhelper_compat_level(
            context.tree.abspath('.'))
    if debhelper_compat_version is not None and debhelper_compat_version < 10:
        def add_with_autoreconf(line, target):
            if target != b'%':
                return line
            if not line.startswith(b'dh '):
                return line
            return dh_invoke_add_with(line, b'autoreconf')

        if update_rules(command_line_cb=add_with_autoreconf):
            return context.add_dependency('dh-autoreconf')

    return False


def fix_missing_configure(error, context):
    if (not context.tree.has_filename('configure.ac') and
            not context.tree.has_filename('configure.in')):
        return False

    return enable_dh_autoreconf(context)


def fix_missing_automake_input(error, context):
    # TODO(jelmer): If it's ./NEWS, ./AUTHORS or ./README that's missing, then
    # try to set 'export AUTOMAKE = automake --foreign' in debian/rules.
    # https://salsa.debian.org/jelmer/debian-janitor/issues/88
    return enable_dh_autoreconf(context)


def fix_missing_maven_artifacts(error, context):
    artifact = error.artifacts[0]
    parts = artifact.split(':')
    if len(parts) == 4:
        (group_id, artifact_id, kind, version) = parts
        regex = False
    elif len(parts) == 3:
        (group_id, artifact_id, version) = parts
        kind = 'jar'
        regex = False
    elif len(parts) == 2:
        version = '.*'
        (group_id, artifact_id) = parts
        kind = 'jar'
        regex = True
    else:
        raise AssertionError(
            'invalid number of parts to artifact %s' % artifact)
    paths = [os.path.join(
        '/usr/share/maven-repo', group_id.replace('.', '/'),
        artifact_id, version, '%s-%s.%s' % (artifact_id, version, kind))]
    package = get_package_for_paths(paths, regex=regex)
    if package is None:
        logging.warning('no package for artifact %s', artifact)
        return False
    return context.add_dependency(package)


def install_gnome_common(error, context):
    return context.add_dependency('gnome-common')


def install_gnome_common_dep(error, context):
    if error.package == 'glib-gettext':
        package = get_package_for_paths(['/usr/bin/glib-gettextize'])
    else:
        package = None
    if package is None:
        logging.warning('No debian package for package %s', error.package)
        return False
    return context.add_dependency(
        package=package,
        minimum_version=error.minimum_version)


def install_xfce_dep(error, context):
    if error.package == 'gtk-doc':
        package = get_package_for_paths(['/usr/bin/gtkdocize'])
    else:
        package = None
    if package is None:
        logging.warning('No debian package for package %s', error.package)
        return False
    return context.add_dependency(package=package)


def fix_missing_config_status_input(error, context):
    autogen_path = 'autogen.sh'
    rules_path = 'debian/rules'
    if context.subpath not in ('.', ''):
        autogen_path = os.path.join(context.subpath, autogen_path)
        rules_path = os.path.join(context.subpath, rules_path)
    if not context.tree.has_filename(autogen_path):
        return False

    def add_autogen(mf):
        rule = any(mf.iter_rules(b'override_dh_autoreconf'))
        if rule:
            return
        rule = mf.add_rule(b'override_dh_autoreconf')
        rule.append_command(b'dh_autoreconf ./autogen.sh')

    if not update_rules(makefile_cb=add_autogen, path=rules_path):
        return False

    if context.update_changelog:
        commit_debian_changes(
            context.tree, context.subpath,
            'Run autogen.sh during build.', committer=context.committer,
            update_changelog=context.update_changelog)

    return True


def _find_aclocal_fun(macro):
    # TODO(jelmer): Use the API for codesearch.debian.net instead?
    defun_prefix = b'AC_DEFUN([%s],' % macro.encode('ascii')
    for entry in os.scandir('/usr/share/aclocal'):
        if not entry.is_file():
            continue
        with open(entry.path, 'rb') as f:
            for line in f:
                if line.startswith(defun_prefix):
                    return entry.path
    raise KeyError


def run_pgbuildext_updatecontrol(error, context):
    logging.info("Running 'pg_buildext updatecontrol'")
    # TODO(jelmer): run in the schroot
    pg_buildext_updatecontrol(context.tree.abspath(context.subpath))
    return commit_debian_changes(
        context.tree, context.subpath, "Run 'pgbuildext updatecontrol'.",
        committer=context.committer, update_changelog=False)


def fix_missing_autoconf_macro(error, context):
    try:
        path = _find_aclocal_fun(error.macro)
    except KeyError:
        logging.info('No local m4 file found defining %s', error.macro)
        return False
    package = get_package_for_paths([path])
    if package is None:
        logging.warning('no package for macro file %s', path)
        return False
    return context.add_dependency(package)


def fix_missing_c_sharp_compiler(error, context):
    return context.add_dependency('mono-mcs')


def fix_missing_haskell_dependencies(error, context):
    path = "/var/lib/ghc/package.conf.d/%s-.*.conf" % error.deps[0][0]
    package = get_package_for_paths([path], regex=True)
    if package is None:
        logging.warning('no package for macro file %s', path)
        return False
    return context.add_dependency(package)


VERSIONED_PACKAGE_FIXERS: List[
        Tuple[Type[Problem], Callable[[Problem, DependencyContext], bool]]] = [
    (NeedPgBuildExtUpdateControl, run_pgbuildext_updatecontrol),
    (MissingConfigure, fix_missing_configure),
    (MissingAutomakeInput, fix_missing_automake_input),
]


APT_FIXERS: List[
        Tuple[Type[Problem], Callable[[Problem, DependencyContext], bool]]] = [
    (MissingPythonModule, fix_missing_python_module),
    (MissingPythonDistribution, fix_missing_python_distribution),
    (MissingCHeader, fix_missing_c_header),
    (MissingPkgConfig, fix_missing_pkg_config),
    (MissingCommand, fix_missing_command),
    (MissingFile, fix_missing_file),
    (MissingSprocketsFile, fix_missing_sprockets_file),
    (MissingGoPackage, fix_missing_go_package),
    (MissingPerlFile, fix_missing_perl_file),
    (MissingPerlModule, fix_missing_perl_file),
    (MissingXmlEntity, fix_missing_xml_entity),
    (MissingNodeModule, fix_missing_node_module),
    (MissingRubyGem, fix_missing_ruby_gem),
    (MissingRPackage, fix_missing_r_package),
    (MissingLibrary, fix_missing_library),
    (MissingJavaClass, fix_missing_java_class),
    (DhAddonLoadFailure, fix_missing_dh_addon),
    (MissingPhpClass, fix_missing_php_class),
    (AptFetchFailure, retry_apt_failure),
    (MissingMavenArtifacts, fix_missing_maven_artifacts),
    (GnomeCommonMissing, install_gnome_common),
    (MissingGnomeCommonDependency, install_gnome_common_dep),
    (MissingXfceDependency, install_xfce_dep),
    (MissingConfigStatusInput, fix_missing_config_status_input),
    (MissingJDKFile, fix_missing_jdk_file),
    (MissingRubyFile, fix_missing_ruby_file),
    (MissingJavaScriptRuntime, fix_missing_javascript_runtime),
    (MissingAutoconfMacro, fix_missing_autoconf_macro),
    (MissingValaPackage, fix_missing_vala_package),
    (MissingCSharpCompiler, fix_missing_c_sharp_compiler),
    (MissingHaskellDependencies, fix_missing_haskell_dependencies),
]


def resolve_error(error, context, fixers):
    relevant_fixers = []
    for error_cls, fixer in fixers:
        if isinstance(error, error_cls):
            relevant_fixers.append(fixer)
    if not relevant_fixers:
        logging.warning('No fixer found for %r', error)
        return False
    for fixer in relevant_fixers:
        logging.info(
            'Attempting to use fixer %r to address %r',
            fixer, error)
        try:
            made_changes = fixer(error, context)
        except GeneratedFile:
            logging.warning('Control file is generated, unable to edit.')
            return False
        if made_changes:
            return True
    return False


def build_incrementally(
        local_tree, suffix, build_suite, output_directory, build_command,
        build_changelog_entry='Build for debian-janitor apt repository.',
        committer=None, max_iterations=DEFAULT_MAX_ITERATIONS,
        subpath='', source_date_epoch=None, update_changelog=True):
    fixed_errors = []
    while True:
        try:
            return attempt_build(
                local_tree, suffix, build_suite, output_directory,
                build_command, build_changelog_entry, subpath=subpath,
                source_date_epoch=source_date_epoch)
        except SbuildFailure as e:
            if e.error is None:
                logging.warning(
                    'Build failed with unidentified error. Giving up.')
                raise
            if e.context is None:
                logging.info('No relevant context, not making any changes.')
                raise
            if (e.error, e.context) in fixed_errors:
                logging.warning(
                    'Error was still not fixed on second try. Giving up.')
                raise
            if max_iterations is not None \
                    and len(fixed_errors) > max_iterations:
                logging.warning(
                    'Last fix did not address the issue. Giving up.')
                raise
            reset_tree(local_tree, local_tree.basis_tree(), subpath=subpath)
            if e.context[0] == 'build':
                context = BuildDependencyContext(
                    local_tree, subpath=subpath, committer=committer,
                    update_changelog=update_changelog)
            elif e.context[0] == 'autopkgtest':
                context = AutopkgtestDependencyContext(
                    e.context[1],
                    local_tree, subpath=subpath, committer=committer,
                    update_changelog=update_changelog)
            else:
                logging.warning('unable to install for context %r', e.context)
                raise
            try:
                if not resolve_error(
                        e.error, context,
                        VERSIONED_PACKAGE_FIXERS + APT_FIXERS):
                    logging.warning(
                        'Failed to resolve error %r. Giving up.', e.error)
                    raise
            except CircularDependency:
                logging.warning(
                    'Unable to fix %r; it would introduce a circular '
                    'dependency.', e.error)
                raise e
            fixed_errors.append((e.error, e.context))
            if os.path.exists(os.path.join(output_directory, 'build.log')):
                i = 1
                while os.path.exists(
                        os.path.join(output_directory, 'build.log.%d' % i)):
                    i += 1
                os.rename(os.path.join(output_directory, 'build.log'),
                          os.path.join(output_directory, 'build.log.%d' % i))


def main(argv=None):
    import argparse
    parser = argparse.ArgumentParser('janitor.fix_build')
    parser.add_argument('--suffix', type=str,
                        help="Suffix to use for test builds.",
                        default='fixbuild1')
    parser.add_argument('--suite', type=str,
                        help="Suite to target.",
                        default='unstable')
    parser.add_argument('--output-directory', type=str,
                        help="Output directory.", default=None)
    parser.add_argument('--committer', type=str,
                        help='Committer string (name and email)',
                        default=None)
    parser.add_argument(
        '--build-command', type=str,
        help='Build command',
        default=(DEFAULT_BUILDER + ' -A -s -v'))
    parser.add_argument(
        '--no-update-changelog', action="store_false", default=None,
        dest="update_changelog", help="do not update the changelog")
    parser.add_argument(
        '--update-changelog', action="store_true", dest="update_changelog",
        help="force updating of the changelog", default=None)

    args = parser.parse_args()
    from breezy.workingtree import WorkingTree
    tree = WorkingTree.open('.')
    build_incrementally(
        tree, args.suffix, args.suite, args.output_directory,
        args.build_command, committer=args.committer,
        update_changelog=args.update_changelog)


if __name__ == '__main__':
    sys.exit(main(sys.argv))

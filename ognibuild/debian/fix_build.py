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
    "build_incrementally",
]

import logging
import os
import sys
from typing import List, Set, Optional

from debian.deb822 import (
    Deb822,
    PkgRelation,
)
from debian.changelog import Version

from breezy.commit import PointlessCommit
from breezy.mutabletree import MutableTree
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

from debmutate._rules import (
    dh_invoke_add_with,
    update_rules,
)

from breezy.plugins.debian.changelog import debcommit
from buildlog_consultant import Problem
from buildlog_consultant.common import (
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
    DhAddonLoadFailure,
    MissingMavenArtifacts,
    GnomeCommonMissing,
    MissingGnomeCommonDependency,
)
from buildlog_consultant.apt import (
    AptFetchFailure,
)
from buildlog_consultant.sbuild import (
    SbuildFailure,
)

from .apt import LocalAptManager
from ..fix_build import BuildFixer, SimpleBuildFixer, resolve_error, DependencyContext
from ..resolver.apt import (
    NoAptPackage,
    get_package_for_python_module,
    )
from ..requirements import (
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
from .build import attempt_build, DEFAULT_BUILDER


DEFAULT_MAX_ITERATIONS = 10


class CircularDependency(Exception):
    """Adding dependency would introduce cycle."""

    def __init__(self, package):
        self.package = package


class BuildDependencyContext(DependencyContext):
    def add_dependency(self, package: str, minimum_version: Optional[Version] = None):
        return add_build_dependency(
            self.tree,
            package,
            minimum_version=minimum_version,
            committer=self.committer,
            subpath=self.subpath,
            update_changelog=self.update_changelog,
        )


class AutopkgtestDependencyContext(DependencyContext):
    def __init__(
        self, testname, tree, apt, subpath="", committer=None, update_changelog=True
    ):
        self.testname = testname
        super(AutopkgtestDependencyContext, self).__init__(
            tree, apt, subpath, committer, update_changelog
        )

    def add_dependency(self, package, minimum_version=None):
        return add_test_dependency(
            self.tree,
            self.testname,
            package,
            minimum_version=minimum_version,
            committer=self.committer,
            subpath=self.subpath,
            update_changelog=self.update_changelog,
        )


def add_build_dependency(
    tree: Tree,
    package: str,
    minimum_version: Optional[Version] = None,
    committer: Optional[str] = None,
    subpath: str = "",
    update_changelog: bool = True,
):
    if not isinstance(package, str):
        raise TypeError(package)

    control_path = os.path.join(tree.abspath(subpath), "debian/control")
    try:
        with ControlEditor(path=control_path) as updater:
            for binary in updater.binaries:
                if binary["Package"] == package:
                    raise CircularDependency(package)
            if minimum_version:
                updater.source["Build-Depends"] = ensure_minimum_version(
                    updater.source.get("Build-Depends", ""), package, minimum_version
                )
            else:
                updater.source["Build-Depends"] = ensure_some_version(
                    updater.source.get("Build-Depends", ""), package
                )
    except FormattingUnpreservable as e:
        logging.info("Unable to edit %s in a way that preserves formatting.", e.path)
        return False

    if minimum_version:
        desc = "%s (>= %s)" % (package, minimum_version)
    else:
        desc = package

    if not updater.changed:
        logging.info("Giving up; dependency %s was already present.", desc)
        return False

    logging.info("Adding build dependency: %s", desc)
    return commit_debian_changes(
        tree,
        subpath,
        "Add missing build dependency on %s." % desc,
        committer=committer,
        update_changelog=update_changelog,
    )


def add_test_dependency(
    tree,
    testname,
    package,
    minimum_version=None,
    committer=None,
    subpath="",
    update_changelog=True,
):
    if not isinstance(package, str):
        raise TypeError(package)

    tests_control_path = os.path.join(tree.abspath(subpath), "debian/tests/control")

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
                        control.get("Depends", ""), package, minimum_version
                    )
                else:
                    control["Depends"] = ensure_some_version(
                        control.get("Depends", ""), package
                    )
    except FormattingUnpreservable as e:
        logging.info("Unable to edit %s in a way that preserves formatting.", e.path)
        return False
    if not updater.changed:
        return False

    if minimum_version:
        desc = "%s (>= %s)" % (package, minimum_version)
    else:
        desc = package

    logging.info("Adding dependency to test %s: %s", testname, desc)
    return commit_debian_changes(
        tree,
        subpath,
        "Add missing dependency for test %s on %s." % (testname, desc),
        update_changelog=update_changelog,
    )


def commit_debian_changes(
    tree: MutableTree,
    subpath: str,
    summary: str,
    committer: Optional[str] = None,
    update_changelog: bool = True,
) -> bool:
    with tree.lock_write():
        try:
            if update_changelog:
                add_changelog_entry(
                    tree, os.path.join(subpath, "debian/changelog"), [summary]
                )
                debcommit(tree, committer=committer, subpath=subpath)
            else:
                tree.commit(
                    message=summary, committer=committer, specific_files=[subpath]
                )
        except PointlessCommit:
            return False
        else:
            return True


def targeted_python_versions(tree: Tree) -> Set[str]:
    with tree.get_file("debian/control") as f:
        control = Deb822(f)
    build_depends = PkgRelation.parse_relations(control.get("Build-Depends", ""))
    all_build_deps: Set[str] = set()
    for or_deps in build_depends:
        all_build_deps.update(or_dep["name"] for or_dep in or_deps)
    targeted = set()
    if any(x.startswith("pypy") for x in all_build_deps):
        targeted.add("pypy")
    if any(x.startswith("python-") for x in all_build_deps):
        targeted.add("cpython2")
    if any(x.startswith("python3-") for x in all_build_deps):
        targeted.add("cpython3")
    return targeted


def fix_missing_python_distribution(error, context):  # noqa: C901
    targeted = targeted_python_versions(context.tree)
    default = not targeted

    pypy_pkg = context.apt.get_package_for_paths(
        ["/usr/lib/pypy/dist-packages/%s-.*.egg-info" % error.distribution], regex=True
    )
    if pypy_pkg is None:
        pypy_pkg = "pypy-%s" % error.distribution
        if not context.apt.package_exists(pypy_pkg):
            pypy_pkg = None

    py2_pkg = context.apt.get_package_for_paths(
        ["/usr/lib/python2\\.[0-9]/dist-packages/%s-.*.egg-info" % error.distribution],
        regex=True,
    )
    if py2_pkg is None:
        py2_pkg = "python-%s" % error.distribution
        if not context.apt.package_exists(py2_pkg):
            py2_pkg = None

    py3_pkg = context.apt.get_package_for_paths(
        ["/usr/lib/python3/dist-packages/%s-.*.egg-info" % error.distribution],
        regex=True,
    )
    if py3_pkg is None:
        py3_pkg = "python3-%s" % error.distribution
        if not context.apt.package_exists(py3_pkg):
            py3_pkg = None

    extra_build_deps = []
    if error.python_version == 2:
        if "pypy" in targeted:
            if not pypy_pkg:
                logging.warning("no pypy package found for %s", error.module)
            else:
                extra_build_deps.append(pypy_pkg)
        if "cpython2" in targeted or default:
            if not py2_pkg:
                logging.warning("no python 2 package found for %s", error.module)
                return False
            extra_build_deps.append(py2_pkg)
    elif error.python_version == 3:
        if not py3_pkg:
            logging.warning("no python 3 package found for %s", error.module)
            return False
        extra_build_deps.append(py3_pkg)
    else:
        if py3_pkg and ("cpython3" in targeted or default):
            extra_build_deps.append(py3_pkg)
        if py2_pkg and ("cpython2" in targeted or default):
            extra_build_deps.append(py2_pkg)
        if pypy_pkg and "pypy" in targeted:
            extra_build_deps.append(pypy_pkg)

    if not extra_build_deps:
        return False

    for dep_pkg in extra_build_deps:
        assert dep_pkg is not None
        if not context.add_dependency(dep_pkg, minimum_version=error.minimum_version):
            return False
    return True


def fix_missing_python_module(error, context):
    if getattr(context, "tree", None) is not None:
        targeted = targeted_python_versions(context.tree)
    else:
        targeted = set()
    default = not targeted

    pypy_pkg = get_package_for_python_module(context.apt, error.module, "pypy")
    py2_pkg = get_package_for_python_module(context.apt, error.module, "python2")
    py3_pkg = get_package_for_python_module(context.apt, error.module, "python3")

    extra_build_deps = []
    if error.python_version == 2:
        if "pypy" in targeted:
            if not pypy_pkg:
                logging.warning("no pypy package found for %s", error.module)
            else:
                extra_build_deps.append(pypy_pkg)
        if "cpython2" in targeted or default:
            if not py2_pkg:
                logging.warning("no python 2 package found for %s", error.module)
                return False
            extra_build_deps.append(py2_pkg)
    elif error.python_version == 3:
        if not py3_pkg:
            logging.warning("no python 3 package found for %s", error.module)
            return False
        extra_build_deps.append(py3_pkg)
    else:
        if py3_pkg and ("cpython3" in targeted or default):
            extra_build_deps.append(py3_pkg)
        if py2_pkg and ("cpython2" in targeted or default):
            extra_build_deps.append(py2_pkg)
        if pypy_pkg and "pypy" in targeted:
            extra_build_deps.append(pypy_pkg)

    if not extra_build_deps:
        return False

    for dep_pkg in extra_build_deps:
        assert dep_pkg is not None
        if not context.add_dependency(dep_pkg, error.minimum_version):
            return False
    return True


def problem_to_upstream_requirement(problem, context):
    if isinstance(problem, MissingFile):
        return PathRequirement(problem.path)
    elif isinstance(problem, MissingCommand):
        return BinaryRequirement(problem.command)
    elif isinstance(problem, MissingPkgConfig):
        return PkgConfigRequirement(
            problem.module, problem.minimum_version)
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
        # TODO(jelmer): Create multiple HaskellPackageRequirement objects?
        return HaskellPackageRequirement(problem.package)
    elif isinstance(problem, MissingMavenArtifacts):
        # TODO(jelmer): Create multiple MavenArtifactRequirement objects?
        return MavenArtifactRequirement(problem.artifacts)
    elif isinstance(problem, MissingCSharpCompiler):
        return BinaryRequirement('msc')
    elif isinstance(problem, GnomeCommonMissing):
        return GnomeCommonRequirement()
    elif isinstance(problem, MissingJDKFile):
        return JDKFileRequirement(problem.jdk_path, problem.filename)
    elif isinstance(problem, MissingGnomeCommonDependency):
        if problem.package == "glib-gettext":
            return BinaryRequirement('glib-gettextize')
        else:
            logging.warning(
                "No known command for gnome-common dependency %s",
                problem.package)
            return None
    elif isinstance(problem, MissingXfceDependency):
        if problem.package == "gtk-doc":
            return BinaryRequirement("gtkdocize")
        else:
            logging.warning(
                "No known command for xfce dependency %s",
                problem.package)
            return None
    elif isinstance(problem, MissingPerlModule):
        return PerlModuleRequirement(
            module=problem.module,
            filename=problem.filename,
            inc=problem.inc)
    elif isinstance(problem, MissingPerlFile):
        return PerlFileRequirement(filename=problem.filename)
    elif isinstance(problem, MissingAutoconfMacro):
        return AutoconfMacroRequirement(problem.macro)
    elif isinstance(problem, MissingPythonModule):
        return PythonModuleRequirement(
            problem.module,
            python_version=problem.python_version,
            minimum_version=problem.minimum_version)
    elif isinstance(problem, MissingPythonDistribution):
        return PythonPackageRequirement(
            problem.module,
            python_version=problem.python_version,
            minimum_version=problem.minimum_version)
    else:
        return None


class UpstreamRequirementFixer(BuildFixer):

    def fix_missing_requirement(self, error, context):
        req = problem_to_upstream_requirement(error)
        if req is None:
            return False

        try:
            package = context.resolver.resolve(req)
        except NoAptPackage:
            return False
        return context.add_dependency(package)


def retry_apt_failure(error, context):
    return True


def enable_dh_autoreconf(context):
    # Debhelper >= 10 depends on dh-autoreconf and enables autoreconf by
    # default.
    debhelper_compat_version = get_debhelper_compat_level(context.tree.abspath("."))
    if debhelper_compat_version is not None and debhelper_compat_version < 10:

        def add_with_autoreconf(line, target):
            if target != b"%":
                return line
            if not line.startswith(b"dh "):
                return line
            return dh_invoke_add_with(line, b"autoreconf")

        if update_rules(command_line_cb=add_with_autoreconf):
            return context.add_dependency("dh-autoreconf")

    return False


def fix_missing_configure(error, context):
    if (not context.tree.has_filename("configure.ac") and
            not context.tree.has_filename("configure.in")):
        return False

    return enable_dh_autoreconf(context)


def fix_missing_automake_input(error, context):
    # TODO(jelmer): If it's ./NEWS, ./AUTHORS or ./README that's missing, then
    # try to set 'export AUTOMAKE = automake --foreign' in debian/rules.
    # https://salsa.debian.org/jelmer/debian-janitor/issues/88
    return enable_dh_autoreconf(context)


def fix_missing_config_status_input(error, context):
    autogen_path = "autogen.sh"
    rules_path = "debian/rules"
    if context.subpath not in (".", ""):
        autogen_path = os.path.join(context.subpath, autogen_path)
        rules_path = os.path.join(context.subpath, rules_path)
    if not context.tree.has_filename(autogen_path):
        return False

    def add_autogen(mf):
        rule = any(mf.iter_rules(b"override_dh_autoreconf"))
        if rule:
            return
        rule = mf.add_rule(b"override_dh_autoreconf")
        rule.append_command(b"dh_autoreconf ./autogen.sh")

    if not update_rules(makefile_cb=add_autogen, path=rules_path):
        return False

    if context.update_changelog:
        commit_debian_changes(
            context.tree,
            context.subpath,
            "Run autogen.sh during build.",
            committer=context.committer,
            update_changelog=context.update_changelog,
        )

    return True


def run_pgbuildext_updatecontrol(error, context):
    logging.info("Running 'pg_buildext updatecontrol'")
    # TODO(jelmer): run in the schroot
    pg_buildext_updatecontrol(context.tree.abspath(context.subpath))
    return commit_debian_changes(
        context.tree,
        context.subpath,
        "Run 'pgbuildext updatecontrol'.",
        committer=context.committer,
        update_changelog=False,
    )


def fix_missing_makefile_pl(error, context):
    if (
        error.filename == "Makefile.PL"
        and not context.tree.has_filename("Makefile.PL")
        and context.tree.has_filename("dist.ini")
    ):
        # TODO(jelmer): add dist-zilla add-on to debhelper
        raise NotImplementedError
    return False


VERSIONED_PACKAGE_FIXERS: List[BuildFixer] = [
    SimpleBuildFixer(
        NeedPgBuildExtUpdateControl, run_pgbuildext_updatecontrol),
    SimpleBuildFixer(MissingConfigure, fix_missing_configure),
    SimpleBuildFixer(MissingAutomakeInput, fix_missing_automake_input),
    SimpleBuildFixer(MissingConfigStatusInput, fix_missing_config_status_input),
]


APT_FIXERS: List[BuildFixer] = [
    SimpleBuildFixer(MissingPythonModule, fix_missing_python_module),
    SimpleBuildFixer(MissingPythonDistribution, fix_missing_python_distribution),
    SimpleBuildFixer(AptFetchFailure, retry_apt_failure),
    UpstreamRequirementFixer(),
]


GENERIC_FIXERS: List[BuildFixer] = [
    SimpleBuildFixer(MissingPerlFile, fix_missing_makefile_pl),
]


def build_incrementally(
    local_tree,
    apt,
    suffix,
    build_suite,
    output_directory,
    build_command,
    build_changelog_entry="Build for debian-janitor apt repository.",
    committer=None,
    max_iterations=DEFAULT_MAX_ITERATIONS,
    subpath="",
    source_date_epoch=None,
    update_changelog=True,
):
    fixed_errors = []
    while True:
        try:
            return attempt_build(
                local_tree,
                suffix,
                build_suite,
                output_directory,
                build_command,
                build_changelog_entry,
                subpath=subpath,
                source_date_epoch=source_date_epoch,
            )
        except SbuildFailure as e:
            if e.error is None:
                logging.warning("Build failed with unidentified error. Giving up.")
                raise
            if e.phase is None:
                logging.info("No relevant context, not making any changes.")
                raise
            if (e.error, e.phase) in fixed_errors:
                logging.warning("Error was still not fixed on second try. Giving up.")
                raise
            if max_iterations is not None and len(fixed_errors) > max_iterations:
                logging.warning("Last fix did not address the issue. Giving up.")
                raise
            reset_tree(local_tree, local_tree.basis_tree(), subpath=subpath)
            if e.phase[0] == "build":
                context = BuildDependencyContext(
                    local_tree,
                    apt,
                    subpath=subpath,
                    committer=committer,
                    update_changelog=update_changelog,
                )
            elif e.phase[0] == "autopkgtest":
                context = AutopkgtestDependencyContext(
                    e.phase[1],
                    local_tree,
                    apt,
                    subpath=subpath,
                    committer=committer,
                    update_changelog=update_changelog,
                )
            else:
                logging.warning("unable to install for context %r", e.phase)
                raise
            try:
                if not resolve_error(
                    e.error, context, VERSIONED_PACKAGE_FIXERS + APT_FIXERS + GENERIC_FIXERS
                ):
                    logging.warning("Failed to resolve error %r. Giving up.", e.error)
                    raise
            except GeneratedFile:
                logging.warning(
                    "Control file is generated, unable to edit to "
                    "resolver error %r.", e.error)
                raise e
            except CircularDependency:
                logging.warning(
                    "Unable to fix %r; it would introduce a circular " "dependency.",
                    e.error,
                )
                raise e
            fixed_errors.append((e.error, e.phase))
            if os.path.exists(os.path.join(output_directory, "build.log")):
                i = 1
                while os.path.exists(
                    os.path.join(output_directory, "build.log.%d" % i)
                ):
                    i += 1
                os.rename(
                    os.path.join(output_directory, "build.log"),
                    os.path.join(output_directory, "build.log.%d" % i),
                )


def main(argv=None):
    import argparse

    parser = argparse.ArgumentParser("ognibuild.debian.fix_build")
    parser.add_argument(
        "--suffix", type=str, help="Suffix to use for test builds.", default="fixbuild1"
    )
    parser.add_argument(
        "--suite", type=str, help="Suite to target.", default="unstable"
    )
    parser.add_argument(
        "--output-directory", type=str, help="Output directory.", default=None
    )
    parser.add_argument(
        "--committer", type=str, help="Committer string (name and email)", default=None
    )
    parser.add_argument(
        "--build-command",
        type=str,
        help="Build command",
        default=(DEFAULT_BUILDER + " -A -s -v"),
    )
    parser.add_argument(
        "--no-update-changelog",
        action="store_false",
        default=None,
        dest="update_changelog",
        help="do not update the changelog",
    )
    parser.add_argument(
        "--update-changelog",
        action="store_true",
        dest="update_changelog",
        help="force updating of the changelog",
        default=None,
    )

    args = parser.parse_args()
    from breezy.workingtree import WorkingTree

    apt = LocalAptManager()

    tree = WorkingTree.open(".")
    build_incrementally(
        tree,
        apt,
        args.suffix,
        args.suite,
        args.output_directory,
        args.build_command,
        committer=args.committer,
        update_changelog=args.update_changelog,
    )


if __name__ == "__main__":
    sys.exit(main(sys.argv))

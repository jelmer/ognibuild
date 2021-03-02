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
from typing import List, Set, Optional, Type

from debian.deb822 import (
    Deb822,
    PkgRelation,
)

from breezy.commit import PointlessCommit
from breezy.mutabletree import MutableTree
from breezy.tree import Tree
from debmutate.control import (
    ensure_relation,
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

try:
    from breezy.workspace import reset_tree
except ImportError:
    from lintian_brush import reset_tree

from lintian_brush.changelog import (
    add_changelog_entry,
)

from debmutate._rules import (
    dh_invoke_add_with,
    update_rules,
)

from breezy.plugins.debian.changelog import debcommit
from buildlog_consultant import Problem
from buildlog_consultant.apt import (
    AptFetchFailure,
)
from buildlog_consultant.common import (
    MissingConfigStatusInput,
    MissingAutomakeInput,
    MissingConfigure,
    NeedPgBuildExtUpdateControl,
    MissingPythonModule,
    MissingPythonDistribution,
    MissingPerlFile,
)
from buildlog_consultant.sbuild import (
    SbuildFailure,
)

from ..buildlog import problem_to_upstream_requirement
from ..fix_build import BuildFixer, resolve_error, DependencyContext
from ..resolver.apt import (
    AptRequirement,
    get_package_for_python_module,
)
from .build import attempt_build, DEFAULT_BUILDER


DEFAULT_MAX_ITERATIONS = 10


class CircularDependency(Exception):
    """Adding dependency would introduce cycle."""

    def __init__(self, package):
        self.package = package


class PackageDependencyFixer(BuildFixer):

    def __init__(self, apt_resolver):
        self.apt_resolver = apt_resolver

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.apt_resolver)

    def __str__(self):
        return "upstream requirement fixer(%s)" % self.apt_resolver

    def can_fix(self, error):
        req = problem_to_upstream_requirement(error)
        return req is not None

    def fix(self, error, context):
        reqs = problem_to_upstream_requirement(error)
        if reqs is None:
            return False

        if not isinstance(reqs, list):
            reqs = [reqs]

        changed = False
        for req in reqs:
            package = self.apt_resolver.resolve(req)
            if package is None:
                return False
            if context.phase[0] == "autopkgtest":
                return add_test_dependency(
                    context.tree,
                    context.phase[1],
                    package,
                    committer=context.committer,
                    subpath=context.subpath,
                    update_changelog=context.update_changelog,
                )
            elif context.phase[0] == "build":
                return add_build_dependency(
                    context.tree,
                    package,
                    committer=context.committer,
                    subpath=context.subpath,
                    update_changelog=context.update_changelog,
                )
            else:
                logging.warning('Unknown phase %r', context.phase)
                return False
        return changed


class BuildDependencyContext(DependencyContext):
    def __init__(
        self, phase, tree, apt, subpath="", committer=None, update_changelog=True
    ):
        self.phase = phase
        super(BuildDependencyContext, self).__init__(
            tree, apt, subpath, committer, update_changelog
        )

    def add_dependency(self, requirement: AptRequirement):
        return add_build_dependency(
            self.tree,
            requirement,
            committer=self.committer,
            subpath=self.subpath,
            update_changelog=self.update_changelog,
        )


class AutopkgtestDependencyContext(DependencyContext):
    def __init__(
        self, phase, tree, apt, subpath="", committer=None, update_changelog=True
    ):
        self.phase = phase
        super(AutopkgtestDependencyContext, self).__init__(
            tree, apt, subpath, committer, update_changelog
        )

    def add_dependency(self, requirement):
        return add_test_dependency(
            self.tree,
            self.testname,
            requirement,
            committer=self.committer,
            subpath=self.subpath,
            update_changelog=self.update_changelog,
        )


def add_build_dependency(
    tree: Tree,
    requirement: AptRequirement,
    committer: Optional[str] = None,
    subpath: str = "",
    update_changelog: bool = True,
):
    if not isinstance(requirement, AptRequirement):
        raise TypeError(requirement)

    control_path = os.path.join(tree.abspath(subpath), "debian/control")
    try:
        with ControlEditor(path=control_path) as updater:
            for binary in updater.binaries:
                if requirement.touches_package(binary["Package"]):
                    raise CircularDependency(binary["Package"])
            for rel in requirement.relations:
                updater.source["Build-Depends"] = ensure_relation(
                    updater.source.get("Build-Depends", ""), PkgRelation.str([rel])
                )
    except FormattingUnpreservable as e:
        logging.info("Unable to edit %s in a way that preserves formatting.", e.path)
        return False

    desc = requirement.pkg_relation_str()

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
    requirement,
    committer=None,
    subpath="",
    update_changelog=True,
):
    if not isinstance(requirement, AptRequirement):
        raise TypeError(requirement)

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
                for rel in requirement.relations:
                    control["Depends"] = ensure_relation(
                        control.get("Depends", ""), PkgRelation.str([rel])
                    )
    except FormattingUnpreservable as e:
        logging.info("Unable to edit %s in a way that preserves formatting.", e.path)
        return False
    if not updater.changed:
        return False

    desc = requirement.pkg_relation_str()

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
        if not context.add_dependency(dep_pkg):
            return False
    return True


def fix_missing_python_module(error, context):
    if getattr(context, "tree", None) is not None:
        targeted = targeted_python_versions(context.tree)
    else:
        targeted = set()
    default = not targeted

    if error.minimum_version:
        specs = [(">=", error.minimum_version)]
    else:
        specs = []

    pypy_pkg = get_package_for_python_module(context.apt, error.module, "pypy", specs)
    py2_pkg = get_package_for_python_module(context.apt, error.module, "python2", specs)
    py3_pkg = get_package_for_python_module(context.apt, error.module, "python3", specs)

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
        if not context.add_dependency(dep_pkg):
            return False
    return True


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
            return context.add_dependency(AptRequirement.simple("dh-autoreconf"))

    return False


def fix_missing_configure(error, context):
    if not context.tree.has_filename("configure.ac") and not context.tree.has_filename(
        "configure.in"
    ):
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


class PgBuildExtOutOfDateControlFixer(BuildFixer):
    def __init__(self, session):
        self.session = session

    def can_fix(self, problem):
        return isinstance(problem, NeedPgBuildExtUpdateControl)

    def _fix(self, error, context):
        logging.info("Running 'pg_buildext updatecontrol'")
        self.session.check_call(["pg_buildext", "updatecontrol"])
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


class SimpleBuildFixer(BuildFixer):
    def __init__(self, problem_cls: Type[Problem], fn):
        self._problem_cls = problem_cls
        self._fn = fn

    def __repr__(self):
        return "%s(%r, %r)" % (type(self).__name__, self._problem_cls, self._fn)

    def can_fix(self, problem: Problem):
        return isinstance(problem, self._problem_cls)

    def _fix(self, problem: Problem, context):
        return self._fn(problem, context)


def versioned_package_fixers(session):
    return [
        PgBuildExtOutOfDateControlFixer(session),
        SimpleBuildFixer(MissingConfigure, fix_missing_configure),
        SimpleBuildFixer(MissingAutomakeInput, fix_missing_automake_input),
        SimpleBuildFixer(MissingConfigStatusInput, fix_missing_config_status_input),
        SimpleBuildFixer(MissingPerlFile, fix_missing_makefile_pl),
    ]


def apt_fixers(apt) -> List[BuildFixer]:
    from ..resolver.apt import AptResolver
    resolver = AptResolver(apt)
    return [
        SimpleBuildFixer(MissingPythonModule, fix_missing_python_module),
        SimpleBuildFixer(MissingPythonDistribution, fix_missing_python_distribution),
        SimpleBuildFixer(AptFetchFailure, retry_apt_failure),
        PackageDependencyFixer(resolver),
    ]


def build_incrementally(
    local_tree,
    apt,
    suffix,
    build_suite,
    output_directory,
    build_command,
    build_changelog_entry,
    committer=None,
    max_iterations=DEFAULT_MAX_ITERATIONS,
    subpath="",
    source_date_epoch=None,
    update_changelog=True,
):
    fixed_errors = []
    fixers = versioned_package_fixers(apt.session) + apt_fixers(apt)
    logging.info("Using fixers: %r", fixers)
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
                    e.phase,
                    local_tree,
                    apt,
                    subpath=subpath,
                    committer=committer,
                    update_changelog=update_changelog,
                )
            elif e.phase[0] == "autopkgtest":
                context = AutopkgtestDependencyContext(
                    e.phase,
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
                if not resolve_error(e.error, context, fixers):
                    logging.warning("Failed to resolve error %r. Giving up.", e.error)
                    raise
            except GeneratedFile:
                logging.warning(
                    "Control file is generated, unable to edit to "
                    "resolver error %r.",
                    e.error,
                )
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
    from .apt import AptManager
    from ..session.plain import PlainSession
    import tempfile
    import contextlib

    apt = AptManager(PlainSession())

    logging.basicConfig(level=logging.INFO, format="%(message)s")

    with contextlib.ExitStack() as es:
        if args.output_directory is None:
            output_directory = es.enter_context(tempfile.TemporaryDirectory())
            logging.info("Using output directory %s", output_directory)
        else:
            output_directory = args.output_directory

        tree = WorkingTree.open(".")
        build_incrementally(
            tree,
            apt,
            args.suffix,
            args.suite,
            output_directory,
            args.build_command,
            None,
            committer=args.committer,
            update_changelog=args.update_changelog,
        )


if __name__ == "__main__":
    sys.exit(main(sys.argv))

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

from functools import partial
import logging
import os
import shutil
import sys
import time
from typing import List, Set, Optional, Type, Tuple

from debian.deb822 import (
    Deb822,
    PkgRelation,
)

from breezy.commit import PointlessCommit, NullCommitReporter
from breezy.tree import Tree
from breezy.workingtree import WorkingTree

from debmutate.changelog import ChangelogEditor
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

from breezy.workspace import reset_tree

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
    MissingPerlFile,
)
from buildlog_consultant.sbuild import (
    DebcargoUnacceptablePredicate,
    DebcargoUnacceptableComparator,
    )

from .build import (
    DetailedDebianBuildFailure,
    UnidentifiedDebianBuildError,
    )
from ..logs import rotate_logfile
from ..buildlog import problem_to_upstream_requirement
from ..fix_build import BuildFixer, resolve_error
from ..resolver.apt import (
    AptRequirement,
)
from .apt import AptManager
from .build import attempt_build, DEFAULT_BUILDER, BUILD_LOG_FILENAME


DEFAULT_MAX_ITERATIONS = 10


class CircularDependency(Exception):
    """Adding dependency would introduce cycle."""

    def __init__(self, package):
        self.package = package


class DebianPackagingContext:
    def __init__(
        self, tree, subpath, committer, update_changelog, commit_reporter=None
    ):
        self.tree = tree
        self.subpath = subpath
        self.committer = committer
        self.update_changelog = update_changelog
        self.commit_reporter = commit_reporter

    def abspath(self, *parts):
        return self.tree.abspath(os.path.join(self.subpath, *parts))

    def commit(
            self, summary: str,
            update_changelog: Optional[bool] = None) -> bool:
        if update_changelog is None:
            update_changelog = self.update_changelog
        with self.tree.lock_write():
            try:
                if update_changelog:
                    cl_path = self.abspath("debian/changelog")
                    with ChangelogEditor(cl_path) as editor:
                        editor.add_entry([summary])
                    debcommit(
                        self.tree, committer=self.committer,
                        subpath=self.subpath,
                        reporter=self.commit_reporter)
                else:
                    self.tree.commit(
                        message=summary,
                        committer=self.committer,
                        specific_files=[self.subpath],
                        reporter=self.commit_reporter,
                    )
            except PointlessCommit:
                return False
            else:
                return True


class PackageDependencyFixer(BuildFixer):
    def __init__(self, context, apt_resolver):
        self.apt_resolver = apt_resolver
        self.context = context

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.apt_resolver)

    def __str__(self):
        return "upstream requirement fixer(%s)" % self.apt_resolver

    def can_fix(self, error):
        req = problem_to_upstream_requirement(error)
        return req is not None

    def fix(self, error, phase):
        req = problem_to_upstream_requirement(error)
        if req is None:
            return False

        if not isinstance(req, list):
            reqs = [req]
        else:
            reqs = req

        changed = False
        for req in reqs:
            apt_req = self.apt_resolver.resolve(req)
            if apt_req is None:
                return False
            if add_dependency(self.context, phase, apt_req):
                changed = True
        return changed


def add_dependency(context, phase, requirement: AptRequirement):
    if phase[0] == "autopkgtest":
        return add_test_dependency(context, phase[1], requirement)
    elif phase[0] == "build":
        return add_build_dependency(context, requirement)
    elif phase[0] == "buildenv":
        # TODO(jelmer): Actually, we probably just want to install it on the
        # host system?
        logging.warning("Unknown phase %r", phase)
        return False
    else:
        logging.warning("Unknown phase %r", phase)
        return False


def add_build_dependency(context, requirement: AptRequirement):
    if not isinstance(requirement, AptRequirement):
        raise TypeError(requirement)

    if os.path.exists(context.abspath('debian/debcargo.toml')):
        from debmutate.debcargo import DebcargoControlShimEditor
        control = DebcargoControlShimEditor.from_debian_dir(
            context.abspath('debian'))
    else:
        control_path = context.abspath("debian/control")
        control = ControlEditor(path=control_path)

    try:
        with control as updater:
            for binary in updater.binaries:
                if requirement.touches_package(binary["Package"]):
                    raise CircularDependency(binary["Package"])
            for rel in requirement.relations:
                updater.source["Build-Depends"] = ensure_relation(
                    updater.source.get("Build-Depends", ""),
                    PkgRelation.str([rel])
                )
    except FormattingUnpreservable as e:
        logging.info(
            "Unable to edit %s in a way that preserves formatting.", e.path)
        return False

    desc = requirement.pkg_relation_str()

    if not updater.changed:
        logging.info(
            "Giving up; build dependency %s was already present.", desc)
        return False

    logging.info("Adding build dependency: %s", desc)
    return context.commit("Add missing build dependency on %s." % desc)


def add_test_dependency(context, testname, requirement):
    if not isinstance(requirement, AptRequirement):
        raise TypeError(requirement)

    tests_control_path = context.abspath("debian/tests/control")

    # TODO(jelmer): If requirement is for one of our binary packages
    # but "@" is already present then don't do anything.

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
        logging.info(
            "Unable to edit %s in a way that preserves formatting.", e.path)
        return False

    desc = requirement.pkg_relation_str()

    if not updater.changed:
        logging.info(
            "Giving up; dependency %s for test %s was already present.",
            desc, testname)
        return False

    logging.info("Adding dependency to test %s: %s", testname, desc)
    return context.commit(
        "Add missing dependency for test %s on %s." % (testname, desc),
    )


def targeted_python_versions(tree: Tree, subpath: str) -> List[str]:
    with tree.get_file(os.path.join(subpath, "debian/control")) as f:
        control = Deb822(f)
    build_depends = PkgRelation.parse_relations(
        control.get("Build-Depends", ""))
    all_build_deps: Set[str] = set()
    for or_deps in build_depends:
        all_build_deps.update(or_dep["name"] for or_dep in or_deps)
    targeted = []
    if any(x.startswith("python3-") for x in all_build_deps):
        targeted.append("python3")
    if any(x.startswith("pypy") for x in all_build_deps):
        targeted.append("pypy")
    if any(x.startswith("python-") for x in all_build_deps):
        targeted.append("python")
    return targeted


def python_tie_breaker(tree, subpath, reqs):
    targeted = targeted_python_versions(tree, subpath)
    if not targeted:
        return None

    def same(pkg, python_version):
        if pkg.startswith(python_version + "-"):
            return True
        if pkg.startswith("lib%s-" % python_version):
            return True
        return pkg == r'lib%s-dev' % python_version

    for python_version in targeted:
        for req in reqs:
            if any(same(name, python_version) for name in req.package_names()):
                logging.info(
                    "Breaking tie between %r to %r, since package already "
                    "has %r build-dependencies",
                    [str(req) for req in reqs],
                    str(req),
                    python_version,
                )
                return req
    return None


def retry_apt_failure(error, phase, apt, context):
    return True


def enable_dh_autoreconf(context, phase):
    # Debhelper >= 10 depends on dh-autoreconf and enables autoreconf by
    # default.
    debhelper_compat_version = get_debhelper_compat_level(
        context.tree.abspath("."))
    if debhelper_compat_version is not None and debhelper_compat_version < 10:

        def add_with_autoreconf(line, target):
            if target != b"%":
                return line
            if not line.startswith(b"dh "):
                return line
            return dh_invoke_add_with(line, b"autoreconf")

        if update_rules(command_line_cb=add_with_autoreconf):
            return add_dependency(
                context, phase, AptRequirement.simple("dh-autoreconf")
            )

    return False


def fix_missing_configure(error, phase, context):
    if (not context.tree.has_filename("configure.ac")
            and not context.tree.has_filename("configure.in")):
        return False

    return enable_dh_autoreconf(context, phase)


def fix_missing_automake_input(error, phase, context):
    # TODO(jelmer): If it's ./NEWS, ./AUTHORS or ./README that's missing, then
    # try to set 'export AUTOMAKE = automake --foreign' in debian/rules.
    # https://salsa.debian.org/jelmer/debian-janitor/issues/88
    return enable_dh_autoreconf(context, phase)


def fix_missing_config_status_input(error, phase, context):
    autogen_path = "autogen.sh"
    rules_path = "debian/rules"
    if context.subpath not in (".", ""):
        autogen_path = os.path.join(context.subpath, autogen_path)
        rules_path = os.path.join(context.subpath, rules_path)
    if not context.tree.has_filename(autogen_path):
        return False

    def add_autogen(mf):
        rule_exists = any(mf.iter_rules(b"override_dh_autoreconf"))
        if rule_exists:
            return
        rule = mf.add_rule(b"override_dh_autoreconf")
        rule.append_command(b"dh_autoreconf ./autogen.sh")

    if not update_rules(makefile_cb=add_autogen, path=rules_path):
        return False

    return context.commit("Run autogen.sh during build.")


class PgBuildExtOutOfDateControlFixer(BuildFixer):
    def __init__(self, packaging_context, session, apt):
        self.session = session
        self.context = packaging_context
        self.apt = apt

    def can_fix(self, problem):
        return isinstance(problem, NeedPgBuildExtUpdateControl)

    def __repr__(self):
        return "%s()" % (type(self).__name__,)

    def _fix(self, error, phase):
        logging.info("Running 'pg_buildext updatecontrol'")
        self.apt.install(['postgresql-common'])
        external_dir, internal_dir = self.session.setup_from_vcs(
            self.context.tree, include_controldir=None,
            subdir=self.context.subpath)
        self.session.chdir(internal_dir)
        self.session.check_call(["pg_buildext", "updatecontrol"])
        shutil.copy(
            os.path.join(external_dir, error.generated_path),
            self.context.abspath(error.generated_path)
        )
        return self.context.commit(
            "Run 'pgbuildext updatecontrol'.", update_changelog=False
        )


def fix_missing_makefile_pl(error, phase, context):
    if (
        error.filename == "Makefile.PL"
        and not context.tree.has_filename("Makefile.PL")
        and context.tree.has_filename("dist.ini")
    ):
        # TODO(jelmer): add dist-zilla add-on to debhelper
        raise NotImplementedError
    return False


def debcargo_coerce_unacceptable_prerelease(error, phase, context):
    from debmutate.debcargo import DebcargoEditor
    with DebcargoEditor(context.abspath('debian/debcargo.toml')) as editor:
        editor['allow_prerelease_deps'] = True
    return context.commit('Enable allow_prerelease_deps.')


class SimpleBuildFixer(BuildFixer):
    def __init__(self, packaging_context, problem_cls: Type[Problem], fn):
        self.context = packaging_context
        self._problem_cls = problem_cls
        self._fn = fn

    def __repr__(self):
        return "%s(%s, %s)" % (
            type(self).__name__,
            self._problem_cls.__name__,
            self._fn.__name__,
        )

    def can_fix(self, problem: Problem):
        return isinstance(problem, self._problem_cls)

    def _fix(self, problem: Problem, phase):
        return self._fn(problem, phase, self.context)


class DependencyBuildFixer(BuildFixer):
    def __init__(self, packaging_context, apt_resolver,
                 problem_cls: Type[Problem], fn):
        self.context = packaging_context
        self.apt_resolver = apt_resolver
        self._problem_cls = problem_cls
        self._fn = fn

    def __repr__(self):
        return "%s(%s, %s)" % (
            type(self).__name__,
            self._problem_cls.__name__,
            self._fn.__name__,
        )

    def can_fix(self, problem: Problem):
        return isinstance(problem, self._problem_cls)

    def _fix(self, problem: Problem, phase):
        return self._fn(problem, phase, self.apt_resolver, self.context)


def versioned_package_fixers(session, packaging_context, apt: AptManager):
    return [
        PgBuildExtOutOfDateControlFixer(packaging_context, session, apt),
        SimpleBuildFixer(
            packaging_context, MissingConfigure, fix_missing_configure),
        SimpleBuildFixer(
            packaging_context, MissingAutomakeInput, fix_missing_automake_input
        ),
        SimpleBuildFixer(
            packaging_context, MissingConfigStatusInput,
            fix_missing_config_status_input
        ),
        SimpleBuildFixer(
            packaging_context, MissingPerlFile, fix_missing_makefile_pl),
        SimpleBuildFixer(
            packaging_context, DebcargoUnacceptablePredicate,
            debcargo_coerce_unacceptable_prerelease),
        SimpleBuildFixer(
            packaging_context, DebcargoUnacceptableComparator,
            debcargo_coerce_unacceptable_prerelease),
    ]


def apt_fixers(apt: AptManager, packaging_context,
               dep_server_url: Optional[str] = None) -> List[BuildFixer]:
    from ..resolver.apt import AptResolver
    from .udd import popcon_tie_breaker
    from .build_deps import BuildDependencyTieBreaker

    apt_tie_breakers = [
        partial(python_tie_breaker, packaging_context.tree,
                packaging_context.subpath),
        BuildDependencyTieBreaker.from_session(apt.session),
        popcon_tie_breaker,
    ]
    resolver: AptResolver
    if dep_server_url:
        from ..resolver.dep_server import DepServerAptResolver
        resolver = DepServerAptResolver(apt, dep_server_url, apt_tie_breakers)
    else:
        resolver = AptResolver(apt, apt_tie_breakers)
    return [
        DependencyBuildFixer(
            packaging_context, apt, AptFetchFailure, retry_apt_failure
        ),
        PackageDependencyFixer(packaging_context, resolver),
    ]


def default_fixers(
        local_tree: WorkingTree,
        subpath: str, apt: AptManager,
        committer: Optional[str] = None,
        update_changelog: Optional[bool] = None,
        dep_server_url: Optional[str] = None):
    packaging_context = DebianPackagingContext(
        local_tree, subpath, committer, update_changelog,
        commit_reporter=NullCommitReporter()
    )
    return (versioned_package_fixers(apt.session, packaging_context, apt)
            + apt_fixers(apt, packaging_context, dep_server_url))


def build_incrementally(
    local_tree: WorkingTree,
    apt: AptManager,
    suffix: Optional[str],
    build_suite: Optional[str],
    output_directory: str,
    build_command: str,
    build_changelog_entry,
    committer: Optional[str] = None,
    max_iterations: int = DEFAULT_MAX_ITERATIONS,
    subpath: str = "",
    source_date_epoch=None,
    update_changelog: bool = True,
    apt_repository: Optional[str] = None,
    apt_repository_key: Optional[str] = None,
    extra_repositories: Optional[List[str]] = None,
    fixers: Optional[List[BuildFixer]] = None,
    run_gbp_dch: Optional[bool] = None,
    dep_server_url: Optional[str] = None,
):
    fixed_errors: List[Tuple[Problem, str]] = []
    if fixers is None:
        fixers = default_fixers(
            local_tree, subpath, apt, committer=committer,
            update_changelog=update_changelog,
            dep_server_url=dep_server_url)
    logging.info("Using fixers: %r", fixers)
    if run_gbp_dch is None:
        run_gbp_dch = (update_changelog is False)
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
                run_gbp_dch=run_gbp_dch,
                apt_repository=apt_repository,
                apt_repository_key=apt_repository_key,
                extra_repositories=extra_repositories,
            )
        except UnidentifiedDebianBuildError:
            logging.warning("Build failed with unidentified error. Giving up.")
            raise
        except DetailedDebianBuildFailure as e:
            if e.phase is None:
                logging.info("No relevant context, not making any changes.")
                raise
            if (e.error, e.phase) in fixed_errors:
                logging.warning(
                    "Error was still not fixed on second try. Giving up.")
                raise
            if (max_iterations is not None
                    and len(fixed_errors) > max_iterations):
                logging.warning(
                    "Last fix did not address the issue. Giving up.")
                raise
            reset_tree(local_tree, subpath=subpath)
            try:
                if not resolve_error(e.error, e.phase, fixers):
                    logging.warning(
                        "Failed to resolve error %r. Giving up.", e.error)
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
                    "Unable to fix %r; it would introduce a circular "
                    "dependency.",
                    e.error,
                )
                raise e
            fixed_errors.append((e.error, e.phase))
            rotate_logfile(os.path.join(output_directory, BUILD_LOG_FILENAME))


def main(argv=None):
    import argparse

    parser = argparse.ArgumentParser("ognibuild.debian.fix_build")
    modifications = parser.add_argument_group('Modifications')
    modifications.add_argument(
        "--suffix", type=str, help="Suffix to use for test builds.",
        default="fixbuild1"
    )
    modifications.add_argument(
        "--suite", type=str, help="Suite to target.", default="unstable"
    )
    modifications.add_argument(
        "--committer", type=str, help="Committer string (name and email)",
        default=None
    )
    modifications.add_argument(
        "--no-update-changelog",
        action="store_false",
        default=None,
        dest="update_changelog",
        help="do not update the changelog",
    )
    modifications.add_argument(
        "--update-changelog",
        action="store_true",
        dest="update_changelog",
        help="force updating of the changelog",
        default=None,
    )
    build_behaviour = parser.add_argument_group('Build Behaviour')
    build_behaviour.add_argument(
        "--output-directory", type=str, help="Output directory.", default=None
    )
    build_behaviour.add_argument(
        "--build-command",
        type=str,
        help="Build command",
        default=(DEFAULT_BUILDER + " -A -s -v"),
    )

    build_behaviour.add_argument(
        '--max-iterations',
        type=int,
        default=DEFAULT_MAX_ITERATIONS,
        help='Maximum number of issues to attempt to fix before giving up.')
    build_behaviour.add_argument("--schroot", type=str, help="chroot to use.")
    parser.add_argument(
        "--dep-server-url", type=str,
        help="ognibuild dep server to use",
        default=os.environ.get('OGNIBUILD_DEPS'))
    parser.add_argument("--verbose", action="store_true", help="Be verbose")

    args = parser.parse_args()
    import breezy.git  # noqa: F401
    import breezy.bzr  # noqa: F401
    from ..session import Session
    from ..session.plain import PlainSession
    from ..session.schroot import SchrootSession
    import tempfile
    import contextlib

    if args.verbose:
        logging.basicConfig(level=logging.DEBUG, format="%(message)s")
    else:
        logging.basicConfig(level=logging.INFO, format="%(message)s")

    with contextlib.ExitStack() as es:
        if args.output_directory is None:
            output_directory = es.enter_context(tempfile.TemporaryDirectory())
            logging.info("Using output directory %s", output_directory)
        else:
            output_directory = args.output_directory
            if not os.path.isdir(output_directory):
                parser.error(
                    'output directory %s is not a directory'
                    % output_directory)

        tree = WorkingTree.open(".")
        session: Session
        if args.schroot:
            session = SchrootSession(args.schroot)
        else:
            session = PlainSession()

        es.enter_context(session)

        apt = AptManager(session)

        try:
            (changes_filenames, cl_entry) = build_incrementally(
                tree,
                apt,
                args.suffix,
                args.suite,
                output_directory,
                args.build_command,
                None,
                committer=args.committer,
                update_changelog=args.update_changelog,
                max_iterations=args.max_iterations,
                dep_server_url=args.dep_server_url,
            )
        except DetailedDebianBuildFailure as e:
            if e.phase is None:
                phase = "unknown phase"
            elif len(e.phase) == 1:
                phase = e.phase[0]
            else:
                phase = "%s (%s)" % (e.phase[0], e.phase[1])
            logging.fatal("Error during %s: %s", phase, e.error)
            if not args.output_directory:
                xdg_cache_dir = os.environ.get(
                    'XDG_CACHE_HOME', os.path.expanduser('~/.cache'))
                buildlogs_dir = os.path.join(
                    xdg_cache_dir, 'ognibuild', 'buildlogs')
                os.makedirs(buildlogs_dir, exist_ok=True)
                target_log_file = os.path.join(
                    buildlogs_dir,
                    '%s-%s.log' % (
                        os.path.basename(getattr(tree, 'basedir', 'build')),
                        time.strftime('%Y-%m-%d_%H%M%s')))
                shutil.copy(
                    os.path.join(output_directory, 'build.log'),
                    target_log_file)
                logging.info('Build log available in %s', target_log_file)
            return 1
        except UnidentifiedDebianBuildError as e:
            if e.phase is None:
                phase = "unknown phase"
            elif len(e.phase) == 1:
                phase = e.phase[0]
            else:
                phase = "%s (%s)" % (e.phase[0], e.phase[1])
            logging.fatal("Error during %s: %s", phase, e.description)
            return 1

        logging.info(
            'Built %s - changes file at %r.',
            cl_entry.version, changes_filenames)


if __name__ == "__main__":
    sys.exit(main(sys.argv))

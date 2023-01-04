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
    "get_build_architecture",
    "version_add_suffix",
    "add_dummy_changelog_entry",
    "build",
    "DetailedDebianBuildFailure",
    "UnidentifiedDebianBuildError",
]

from datetime import datetime
import logging
import os
import re
import shlex
import subprocess
import sys
from typing import Optional, List, Tuple

from debian.changelog import Changelog, Version, ChangeBlock
from debmutate.changelog import get_maintainer, ChangelogEditor
from debmutate.reformatting import GeneratedFile

from breezy.mutabletree import MutableTree
from breezy.plugins.debian.builder import BuildFailedError
from breezy.tree import Tree
from breezy.workingtree import WorkingTree

from buildlog_consultant.sbuild import (
    worker_failure_from_sbuild_log,
    DpkgSourceLocalChanges,
)

from .. import DetailedFailure, UnidentifiedError

BUILD_LOG_FILENAME = 'build.log'

DEFAULT_BUILDER = "sbuild --no-clean-source"


class ChangelogNotEditable(Exception):
    """Changelog can not be edited."""

    def __init__(self, path):
        self.path = path


class DetailedDebianBuildFailure(DetailedFailure):

    def __init__(self, stage, phase, retcode, argv, error, description):
        super(DetailedDebianBuildFailure, self).__init__(retcode, argv, error)
        self.stage = stage
        self.phase = phase
        self.description = description


class UnidentifiedDebianBuildError(UnidentifiedError):

    def __init__(self, stage, phase, retcode, argv, lines, description,
                 secondary=None):
        super(UnidentifiedDebianBuildError, self).__init__(
            retcode, argv, lines, secondary)
        self.stage = stage
        self.phase = phase
        self.description = description


class MissingChangesFile(Exception):
    """Expected changes file was not written."""

    def __init__(self, filename):
        self.filename = filename


def find_changes_files(path: str, package: str, version: Version):
    non_epoch_version = version.upstream_version or ''
    if version.debian_version is not None:
        non_epoch_version += "-%s" % version.debian_version
    c = re.compile('%s_%s_(.*).changes' % (
        re.escape(package), re.escape(non_epoch_version)))
    for entry in os.scandir(path):
        m = c.match(entry.name)
        if m:
            yield m.group(1), entry


def get_build_architecture():
    try:
        return (
            subprocess.check_output(["dpkg-architecture", "-qDEB_BUILD_ARCH"])
            .strip()
            .decode()
        )
    except subprocess.CalledProcessError as e:
        raise Exception("Could not find the build architecture: %s" % e) from e


def control_files_in_root(tree: Tree, subpath: str) -> bool:
    debian_path = os.path.join(subpath, "debian")
    if tree.has_filename(debian_path):
        return False
    control_path = os.path.join(subpath, "control")
    if tree.has_filename(control_path):
        return True
    return tree.has_filename(control_path + ".in")


def version_add_suffix(version: Version, suffix: str) -> Version:
    version = Version(str(version))

    def add_suffix(v):
        m = re.fullmatch("(.*)(" + re.escape(suffix) + ")([0-9]+)", v)
        if m:
            return m.group(1) + m.group(2) + "%d" % (int(m.group(3)) + 1)
        else:
            return v + suffix + "1"
    if version.debian_revision:
        version.debian_revision = add_suffix(version.debian_revision)
    else:
        version.upstream_version = add_suffix(version.upstream_version)
    return version


def add_dummy_changelog_entry(
    tree: MutableTree,
    subpath: str,
    suffix: str,
    suite: str,
    message: str,
    timestamp: Optional[datetime] = None,
    maintainer: Optional[Tuple[Optional[str], Optional[str]]] = None,
    allow_reformatting: bool = True,
) -> Version:
    """Add a dummy changelog entry to a package.

    Args:
      directory: Directory to run in
      suffix: Suffix for the version
      suite: Debian suite
      message: Changelog message
    Returns:
      version of the newly added entry
    """

    if control_files_in_root(tree, subpath):
        path = os.path.join(subpath, "changelog")
    else:
        path = os.path.join(subpath, "debian", "changelog")
    if maintainer is None:
        maintainer = get_maintainer()
    if timestamp is None:
        timestamp = datetime.now()
    try:
        with ChangelogEditor(
                tree.abspath(path),  # type: ignore
                allow_reformatting=allow_reformatting) as editor:
            version = version_add_suffix(editor[0].version, suffix)
            editor.auto_version(version, timestamp=timestamp)
            editor.add_entry(
                summary=[message], maintainer=maintainer, timestamp=timestamp,
                urgency='low')
            editor[0].distributions = suite
            return version
    except GeneratedFile as e:
        raise ChangelogNotEditable(path) from e


def get_latest_changelog_entry(
        local_tree: WorkingTree, subpath: str = "") -> ChangeBlock:
    if control_files_in_root(local_tree, subpath):
        path = os.path.join(subpath, "changelog")
    else:
        path = os.path.join(subpath, "debian", "changelog")
    with local_tree.get_file(path) as f:
        cl = Changelog(f, max_blocks=1)
        return cl[0]


def _builddeb_command(
        build_command: str = DEFAULT_BUILDER,
        result_dir: Optional[str] = None,
        apt_repository: Optional[str] = None,
        apt_repository_key: Optional[str] = None,
        extra_repositories: Optional[List[str]] = None):
    for repo in extra_repositories or []:
        build_command += " --extra-repository=" + shlex.quote(repo)
    args = [
        sys.executable,
        "-m",
        "breezy",
        "builddeb",
        "--guess-upstream-branch-url",
        "--builder=%s" % build_command,
    ]
    if apt_repository:
        args.append("--apt-repository=%s" % apt_repository)
    if apt_repository_key:
        args.append("--apt-repository-key=%s" % apt_repository_key)
    if result_dir:
        args.append("--result-dir=%s" % result_dir)
    return args


def build(
    local_tree: WorkingTree,
    outf,
    build_command: str = DEFAULT_BUILDER,
    result_dir: Optional[str] = None,
    distribution: Optional[str] = None,
    subpath: str = "",
    source_date_epoch: Optional[int] = None,
    apt_repository: Optional[str] = None,
    apt_repository_key: Optional[str] = None,
    extra_repositories: Optional[List[str]] = None,
):
    args = _builddeb_command(
        build_command=build_command,
        result_dir=result_dir,
        apt_repository=apt_repository,
        apt_repository_key=apt_repository_key,
        extra_repositories=extra_repositories)

    outf.write("Running %r\n" % (build_command,))
    outf.flush()
    env = dict(os.environ.items())
    if distribution is not None:
        env["DISTRIBUTION"] = distribution
    if source_date_epoch is not None:
        env["SOURCE_DATE_EPOCH"] = "%d" % source_date_epoch
    logging.info("Building debian packages, running %r.", build_command)
    try:
        subprocess.check_call(
            args, cwd=local_tree.abspath(subpath), stdout=outf, stderr=outf,
            env=env
        )
    except subprocess.CalledProcessError as e:
        raise BuildFailedError() from e


def build_once(
    local_tree: WorkingTree,
    build_suite: Optional[str],
    output_directory: str,
    build_command: str,
    subpath: str = "",
    source_date_epoch: Optional[int] = None,
    apt_repository: Optional[str] = None,
    apt_repository_key: Optional[str] = None,
    extra_repositories: Optional[List[str]] = None
):
    build_log_path = os.path.join(output_directory, BUILD_LOG_FILENAME)
    logging.debug("Writing build log to %s", build_log_path)
    try:
        with open(build_log_path, "w") as f:
            build(
                local_tree,
                outf=f,
                build_command=build_command,
                result_dir=output_directory,
                distribution=build_suite,
                subpath=subpath,
                source_date_epoch=source_date_epoch,
                apt_repository=apt_repository,
                apt_repository_key=apt_repository_key,
                extra_repositories=extra_repositories,
            )
    except BuildFailedError as e:
        with open(build_log_path, "rb") as f:
            sbuild_failure = worker_failure_from_sbuild_log(f)

            # Preserve the diff for later inspection
            # TODO(jelmer): Move this onto a method on DpkgSourceLocalChanges?
            if (isinstance(sbuild_failure.error, DpkgSourceLocalChanges)
                    and getattr(sbuild_failure.error, 'diff_file', None)
                    and os.path.exists(
                        sbuild_failure.error.diff_file)):  # type: ignore
                import shutil
                diff_file = sbuild_failure.error.diff_file  # type: ignore
                shutil.copy(
                    diff_file,
                    os.path.join(output_directory,
                                 os.path.basename(diff_file)))

            retcode = getattr(e, 'returncode', None)
            if sbuild_failure.error:
                raise DetailedDebianBuildFailure(
                    sbuild_failure.stage,
                    sbuild_failure.phase, retcode,
                    shlex.split(build_command),
                    sbuild_failure.error,
                    sbuild_failure.description) from e
            else:
                raise UnidentifiedDebianBuildError(
                    sbuild_failure.stage,
                    sbuild_failure.phase,
                    retcode, shlex.split(build_command),
                    [], sbuild_failure.description) from e

    cl_entry = get_latest_changelog_entry(local_tree, subpath)
    if cl_entry.package is None:
        raise Exception('missing package in changelog entry')
    changes_names = []
    for _kind, entry in find_changes_files(
            output_directory, cl_entry.package, cl_entry.version):
        changes_names.append((entry.name))
    return (changes_names, cl_entry)


class GitBuildpackageMissing(Exception):
    """git-buildpackage is not installed"""


def gbp_dch(path):
    try:
        subprocess.check_call(["gbp", "dch", "--ignore-branch"], cwd=path)
    except FileNotFoundError as e:
        raise GitBuildpackageMissing() from e


def attempt_build(
    local_tree: WorkingTree,
    suffix: Optional[str],
    build_suite: Optional[str],
    output_directory: str,
    build_command: str,
    build_changelog_entry: Optional[str] = None,
    subpath: str = "",
    source_date_epoch: Optional[int] = None,
    run_gbp_dch: bool = False,
    apt_repository: Optional[str] = None,
    apt_repository_key: Optional[str] = None,
    extra_repositories: Optional[List[str]] = None
):
    """Attempt a build, with a custom distribution set.

    Args:
      local_tree: Tree to build in
      suffix: Suffix to add to version string
      build_suite: Name of suite (i.e. distribution) to build for
      output_directory: Directory to write output to
      build_command: Build command to build package
      build_changelog_entry: Changelog entry to use
      subpath: Sub path in tree where package lives
      source_date_epoch: Source date epoch to set
    Returns: Tuple with (changes_name, cl_version)
    """
    if run_gbp_dch and not subpath and hasattr(local_tree.controldir, '_git'):
        gbp_dch(local_tree.abspath(subpath))
    if build_changelog_entry is not None:
        if suffix is None:
            raise AssertionError(
                'build_changelog_entry specified, but suffix is None')
        if build_suite is None:
            raise AssertionError(
                'build_changelog_entry specified, but build_suite is None')
        add_dummy_changelog_entry(
            local_tree, subpath, suffix, build_suite, build_changelog_entry
        )
    return build_once(
        local_tree,
        build_suite,
        output_directory,
        build_command,
        subpath,
        source_date_epoch=source_date_epoch,
        apt_repository=apt_repository,
        apt_repository_key=apt_repository_key,
        extra_repositories=extra_repositories,
    )

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
    "add_dummy_changelog_entry",
    "build",
    "DetailedDebianBuildFailure",
    "UnidentifiedDebianBuildError",
]

from datetime import datetime
from debmutate.changelog import ChangelogEditor
import logging
import os
import re
import shlex
import subprocess
import sys

from debian.changelog import Changelog
from debmutate.changelog import get_maintainer

from breezy.mutabletree import MutableTree
from breezy.plugins.debian.builder import BuildFailedError
from breezy.tree import Tree

from buildlog_consultant.sbuild import (
    worker_failure_from_sbuild_log,
)

from .. import DetailedFailure as DetailedFailure, UnidentifiedError


DEFAULT_BUILDER = "sbuild --no-clean-source"


class DetailedDebianBuildFailure(DetailedFailure):

    def __init__(self, stage, phase, retcode, argv, error, description):
        super(DetailedDebianBuildFailure, self).__init__(retcode, argv, error)
        self.stage = stage
        self.phase = phase
        self.description = description


class UnidentifiedDebianBuildError(UnidentifiedError):

    def __init__(self, stage, phase, retcode, argv, lines, description, secondary=None):
        super(UnidentifiedDebianBuildError, self).__init__(
            retcode, argv, lines, secondary)
        self.stage = stage
        self.phase = phase
        self.description = description


class MissingChangesFile(Exception):
    """Expected changes file was not written."""

    def __init__(self, filename):
        self.filename = filename


def find_changes_files(path, package, version):
    non_epoch_version = version.upstream_version
    if version.debian_version is not None:
        non_epoch_version += "-%s" % version.debian_version
    c = re.compile('%s_%s_(.*).changes' % (re.escape(package), re.escape(non_epoch_version)))
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
        raise Exception("Could not find the build architecture: %s" % e)


def control_files_in_root(tree: Tree, subpath: str) -> bool:
    debian_path = os.path.join(subpath, "debian")
    if tree.has_filename(debian_path):
        return False
    control_path = os.path.join(subpath, "control")
    if tree.has_filename(control_path):
        return True
    if tree.has_filename(control_path + ".in"):
        return True
    return False


def add_dummy_changelog_entry(
    tree: MutableTree,
    subpath: str,
    suffix: str,
    suite: str,
    message: str,
    timestamp=None,
    maintainer=None,
    allow_reformatting: bool = True,
):
    """Add a dummy changelog entry to a package.

    Args:
      directory: Directory to run in
      suffix: Suffix for the version
      suite: Debian suite
      message: Changelog message
    """

    def add_suffix(v, suffix):
        m = re.fullmatch(
            "(.*)(" + re.escape(suffix) + ")([0-9]+)",
            v,
        )
        if m:
            return m.group(1) + m.group(2) + "%d" % (int(m.group(3)) + 1)
        else:
            return v + suffix + "1"

    if control_files_in_root(tree, subpath):
        path = os.path.join(subpath, "changelog")
    else:
        path = os.path.join(subpath, "debian", "changelog")
    if maintainer is None:
        maintainer = get_maintainer()
    if timestamp is None:
        timestamp = datetime.now()
    with ChangelogEditor(
            tree.abspath(os.path.join(path)),  # type: ignore
            allow_reformatting=allow_reformatting) as editor:
        version = editor[0].version
        if version.debian_revision:
            version.debian_revision = add_suffix(version.debian_revision, suffix)
        else:
            version.upstream_version = add_suffix(version.upstream_version, suffix)
        editor.auto_version(version, timestamp=timestamp)
        editor.add_entry(
            summary=[message], maintainer=maintainer, timestamp=timestamp, urgency='low')
        editor[0].distributions = suite


def get_latest_changelog_entry(local_tree, subpath=""):
    if control_files_in_root(local_tree, subpath):
        path = os.path.join(subpath, "changelog")
    else:
        path = os.path.join(subpath, "debian", "changelog")
    with local_tree.get_file(path) as f:
        cl = Changelog(f, max_blocks=1)
        return cl[0]


def build(
    local_tree,
    outf,
    build_command=DEFAULT_BUILDER,
    result_dir=None,
    distribution=None,
    subpath="",
    source_date_epoch=None,
    extra_repositories=None,
):
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
    if result_dir:
        args.append("--result-dir=%s" % result_dir)
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
            args, cwd=local_tree.abspath(subpath), stdout=outf, stderr=outf, env=env
        )
    except subprocess.CalledProcessError:
        raise BuildFailedError()


def build_once(
    local_tree,
    build_suite,
    output_directory,
    build_command,
    subpath="",
    source_date_epoch=None,
    extra_repositories=None
):
    build_log_path = os.path.join(output_directory, "build.log")
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
                extra_repositories=extra_repositories,
            )
    except BuildFailedError as e:
        with open(build_log_path, "rb") as f:
            sbuild_failure = worker_failure_from_sbuild_log(f)
            retcode = getattr(e, 'returncode', None)
            if sbuild_failure.error:
                raise DetailedDebianBuildFailure(
                    sbuild_failure.stage,
                    sbuild_failure.phase, retcode,
                    shlex.split(build_command),
                    sbuild_failure.error,
                    sbuild_failure.description)
            else:
                raise UnidentifiedDebianBuildError(
                    sbuild_failure.stage,
                    sbuild_failure.phase,
                    retcode, shlex.split(build_command),
                    [], sbuild_failure.description)

    cl_entry = get_latest_changelog_entry(local_tree, subpath)
    changes_names = []
    for kind, entry in find_changes_files(output_directory, cl_entry.package, cl_entry.version):
        changes_names.append((entry.name))
    return (changes_names, cl_entry)


class GitBuildpackageMissing(Exception):
    """git-buildpackage is not installed"""


def gbp_dch(path):
    try:
        subprocess.check_call(["gbp", "dch", "--ignore-branch"], cwd=path)
    except FileNotFoundError:
        raise GitBuildpackageMissing()


def attempt_build(
    local_tree,
    suffix,
    build_suite,
    output_directory,
    build_command,
    build_changelog_entry=None,
    subpath="",
    source_date_epoch=None,
    run_gbp_dch=False,
    extra_repositories=None
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
        extra_repositories=extra_repositories,
    )

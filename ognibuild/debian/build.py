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
    "changes_filename",
    "get_build_architecture",
    "add_dummy_changelog_entry",
    "build",
    "SbuildFailure",
]

from datetime import datetime
import logging
import os
import re
import subprocess
import sys

from debian.changelog import Changelog
from debmutate.changelog import get_maintainer, format_datetime

from breezy import osutils
from breezy.mutabletree import MutableTree
from breezy.plugins.debian.builder import BuildFailedError

from buildlog_consultant.sbuild import (
    worker_failure_from_sbuild_log,
    SbuildFailure,
)


DEFAULT_BUILDER = "sbuild --no-clean-source"


class MissingChangesFile(Exception):
    """Expected changes file was not written."""

    def __init__(self, filename):
        self.filename = filename


def changes_filename(package, version, arch):
    non_epoch_version = version.upstream_version
    if version.debian_version is not None:
        non_epoch_version += "-%s" % version.debian_version
    return "%s_%s_%s.changes" % (package, non_epoch_version, arch)


def get_build_architecture():
    try:
        return (
            subprocess.check_output(["dpkg-architecture", "-qDEB_BUILD_ARCH"])
            .strip()
            .decode()
        )
    except subprocess.CalledProcessError as e:
        raise Exception("Could not find the build architecture: %s" % e)


def add_dummy_changelog_entry(
    tree: MutableTree,
    subpath: str,
    suffix: str,
    suite: str,
    message: str,
    timestamp=None,
    maintainer=None,
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

    path = os.path.join(subpath, "debian", "changelog")
    if maintainer is None:
        maintainer = get_maintainer()
    if timestamp is None:
        timestamp = datetime.now()
    with tree.get_file(path) as f:
        cl = Changelog()
        cl.parse_changelog(f, max_blocks=None, allow_empty_author=True, strict=False)
        version = cl[0].version
        if version.debian_revision:
            version.debian_revision = add_suffix(version.debian_revision, suffix)
        else:
            version.upstream_version = add_suffix(version.upstream_version, suffix)
        cl.new_block(
            package=cl[0].package,
            version=version,
            urgency="low",
            distributions=suite,
            author="%s <%s>" % maintainer,
            date=format_datetime(timestamp),
            changes=["", "  * " + message, ""],
        )
    cl_str = cl._format(allow_missing_author=True)
    tree.put_file_bytes_non_atomic(path, cl_str.encode(cl._encoding))


def get_latest_changelog_version(local_tree, subpath=""):
    path = osutils.pathjoin(subpath, "debian/changelog")
    with local_tree.get_file(path) as f:
        cl = Changelog(f, max_blocks=1)
        return cl.package, cl.version


def build(
    local_tree,
    outf,
    build_command=DEFAULT_BUILDER,
    result_dir=None,
    distribution=None,
    subpath="",
    source_date_epoch=None,
):
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
):
    build_log_path = os.path.join(output_directory, "build.log")
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
            )
    except BuildFailedError:
        with open(build_log_path, "rb") as f:
            raise worker_failure_from_sbuild_log(f)

    (cl_package, cl_version) = get_latest_changelog_version(local_tree, subpath)
    changes_name = changes_filename(cl_package, cl_version, get_build_architecture())
    changes_path = os.path.join(output_directory, changes_name)
    if not os.path.exists(changes_path):
        raise MissingChangesFile(changes_name)
    return (changes_name, cl_version)


def gbp_dch(path):
    subprocess.check_call(["gbp", "dch"], cwd=path)


def attempt_build(
    local_tree,
    suffix,
    build_suite,
    output_directory,
    build_command,
    build_changelog_entry=None,
    subpath="",
    source_date_epoch=None,
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
    )

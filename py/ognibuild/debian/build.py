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

import logging
import os
import re
import shlex
import subprocess
import sys
from datetime import datetime
from typing import Optional

from breezy.mutabletree import MutableTree
from breezy.plugins.debian.builder import BuildFailedError
from breezy.tree import Tree
from breezy.workingtree import WorkingTree
from buildlog_consultant.sbuild import (
    DpkgSourceLocalChanges,
    worker_failure_from_sbuild_log,
)
from debian.changelog import ChangeBlock, Changelog, Version
from debmutate.changelog import ChangelogEditor, get_maintainer
from debmutate.reformatting import GeneratedFile

from .. import DetailedFailure, UnidentifiedError

BUILD_LOG_FILENAME = "build.log"

DEFAULT_BUILDER = "sbuild --no-clean-source"


class ChangelogNotEditable(Exception):
    """Changelog can not be edited."""

    def __init__(self, path) -> None:
        self.path = path


class DetailedDebianBuildFailure(DetailedFailure):
    def __init__(
        self, stage, phase, retcode, argv, error, description
    ) -> None:
        super().__init__(retcode, argv, error)
        self.stage = stage
        self.phase = phase
        self.description = description


class UnidentifiedDebianBuildError(UnidentifiedError):
    def __init__(
        self, stage, phase, retcode, argv, lines, description, secondary=None
    ) -> None:
        super().__init__(retcode, argv, lines, secondary)
        self.stage = stage
        self.phase = phase
        self.description = description


class MissingChangesFile(Exception):
    """Expected changes file was not written."""

    def __init__(self, filename) -> None:
        self.filename = filename


def find_changes_files(path: str, package: str, version: Version):
    non_epoch_version = version.upstream_version or ""
    if version.debian_version is not None:
        non_epoch_version += f"-{version.debian_version}"
    c = re.compile(
        f"{re.escape(package)}_{re.escape(non_epoch_version)}_(.*).changes"
    )
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
        raise Exception(f"Could not find the build architecture: {e}") from e


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
    maintainer: Optional[tuple[Optional[str], Optional[str]]] = None,
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
            allow_reformatting=allow_reformatting,
        ) as editor:
            version = version_add_suffix(editor[0].version, suffix)
            logging.debug("Adding dummy changelog entry %s for build", version)
            editor.auto_version(version, timestamp=timestamp)
            editor.add_entry(
                summary=[message],
                maintainer=maintainer,
                timestamp=timestamp,
                urgency="low",
            )
            editor[0].distributions = suite
            return version
    except GeneratedFile as e:
        raise ChangelogNotEditable(path) from e


def get_latest_changelog_entry(
    local_tree: WorkingTree, subpath: str = ""
) -> ChangeBlock:
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
    extra_repositories: Optional[list[str]] = None,
):
    for repo in extra_repositories or []:
        build_command += " --extra-repository=" + shlex.quote(repo)
    args = [
        sys.executable,
        "-m",
        "breezy",
        "builddeb",
        "--guess-upstream-branch-url",
        f"--builder={build_command}",
    ]
    if apt_repository:
        args.append(f"--apt-repository={apt_repository}")
    if apt_repository_key:
        args.append(f"--apt-repository-key={apt_repository_key}")
    if result_dir:
        args.append(f"--result-dir={result_dir}")
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
    extra_repositories: Optional[list[str]] = None,
):
    args = _builddeb_command(
        build_command=build_command,
        result_dir=result_dir,
        apt_repository=apt_repository,
        apt_repository_key=apt_repository_key,
        extra_repositories=extra_repositories,
    )

    outf.write(f"Running {build_command!r}\n")
    outf.flush()
    env = dict(os.environ.items())
    if distribution is not None:
        env["DISTRIBUTION"] = distribution
    if source_date_epoch is not None:
        env["SOURCE_DATE_EPOCH"] = "%d" % source_date_epoch
    logging.info("Building debian packages, running %r.", build_command)
    try:
        subprocess.check_call(
            args,
            cwd=local_tree.abspath(subpath),
            stdout=outf,
            stderr=outf,
            env=env,
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
    extra_repositories: Optional[list[str]] = None,
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
            if (
                isinstance(sbuild_failure.error, DpkgSourceLocalChanges)
                and getattr(sbuild_failure.error, "diff_file", None)
                and os.path.exists(sbuild_failure.error.diff_file)  # type: ignore
            ):
                import shutil

                diff_file: str = sbuild_failure.error.diff_file  # type: ignore
                shutil.copy(
                    diff_file,
                    os.path.join(
                        output_directory, os.path.basename(diff_file)
                    ),
                )

            retcode = getattr(e, "returncode", None)
            if sbuild_failure.error:
                raise DetailedDebianBuildFailure(
                    sbuild_failure.stage,
                    sbuild_failure.phase,
                    retcode,
                    shlex.split(build_command),
                    sbuild_failure.error,
                    sbuild_failure.description,
                ) from e
            else:
                raise UnidentifiedDebianBuildError(
                    sbuild_failure.stage,
                    sbuild_failure.phase,
                    retcode,
                    shlex.split(build_command),
                    [],
                    sbuild_failure.description,
                ) from e

    cl_entry = get_latest_changelog_entry(local_tree, subpath)
    if cl_entry.package is None:
        raise Exception("missing package in changelog entry")
    changes_names = []
    for _kind, entry in find_changes_files(
        output_directory, cl_entry.package, cl_entry.version
    ):
        changes_names.append(entry.name)
    return (changes_names, cl_entry)


class GitBuildpackageMissing(Exception):
    """git-buildpackage is not installed."""


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
    extra_repositories: Optional[list[str]] = None,
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
    if run_gbp_dch and not subpath and hasattr(local_tree.controldir, "_git"):
        gbp_dch(local_tree.abspath(subpath))
    if build_changelog_entry is not None:
        if suffix is None:
            raise AssertionError(
                "build_changelog_entry specified, but suffix is None"
            )
        if build_suite is None:
            raise AssertionError(
                "build_changelog_entry specified, but build_suite is None"
            )
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


def main():
    from argparse import ArgumentParser

    import breezy.bzr  # noqa: F401
    import breezy.git  # noqa: F401
    from breezy.workingtree import WorkingTree

    parser = ArgumentParser()
    parser.add_argument("--suffix", type=str)
    parser.add_argument("--build-command", type=str, default=DEFAULT_BUILDER)
    parser.add_argument("--output-directory", type=str, default="..")
    parser.add_argument("--build-suite", type=str)
    parser.add_argument("--debug", action="store_true")
    parser.add_argument("--build-changelog-entry", type=str)
    args = parser.parse_args()

    wt, subpath = WorkingTree.open_containing(".")

    if args.debug:
        level = logging.DEBUG
    else:
        level = logging.INFO

    logging.basicConfig(format="%(message)s", level=level)

    logging.info("Using output directory %s", args.output_directory)

    if args.suffix and not args.build_changelog_entry:
        parser.error("--suffix requires --build-changelog-entry")

    if args.build_changelog_entry and not args.build_suite:
        parser.error("--build-changelog-entry requires --build-suite")

    try:
        attempt_build(
            wt,
            subpath=subpath,
            suffix=args.suffix,
            build_command=args.build_command,
            output_directory=args.output_directory,
            build_changelog_entry=args.build_changelog_entry,
            build_suite=args.build_suite,
        )
    except UnidentifiedDebianBuildError as e:
        logging.fatal("build failed during %s: %s", e.phase, e.description)
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())

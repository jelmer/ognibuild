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

import errno
import logging
import os
import shutil
import sys
import tempfile
import time
from typing import Optional

from debian.deb822 import Deb822

from breezy.tree import Tree
from breezy.workingtree import WorkingTree

from buildlog_consultant.common import (
    NoSpaceOnDevice,
)


from . import DetailedFailure
from .buildsystem import NoBuildToolsFound
from .session.schroot import SchrootSession
from .vcs import dupe_vcs_tree, export_vcs_tree


SUPPORTED_DIST_EXTENSIONS = [
    ".tar.gz",
    ".tgz",
    ".tar.bz2",
    ".tar.xz",
    ".tar.lzma",
    ".tbz2",
    ".tar",
    ".zip",
]


def is_dist_file(fn):
    for ext in SUPPORTED_DIST_EXTENSIONS:
        if fn.endswith(ext):
            return True
    return False


class DistNoTarball(Exception):
    """Dist operation did not create a tarball."""


def run_dist(session, buildsystems, resolver, fixers, quiet=False):
    # Some things want to write to the user's home directory,
    # e.g. pip caches in ~/.cache
    session.create_home()

    for buildsystem in buildsystems:
        buildsystem.dist(session, resolver, fixers, quiet=quiet)
        return

    raise NoBuildToolsFound()


class DistCatcher(object):
    def __init__(self, directory):
        self.export_directory = directory
        self.files = []
        self.existing_files = None
        self.start_time = time.time()

    def __enter__(self):
        self.existing_files = os.listdir(self.export_directory)
        return self

    def find_files(self):
        new_files = os.listdir(self.export_directory)
        diff_files = set(new_files) - set(self.existing_files)
        diff = set([n for n in diff_files if is_dist_file(n)])
        if len(diff) == 1:
            fn = diff.pop()
            logging.info("Found tarball %s in package directory.", fn)
            self.files.append(os.path.join(self.export_directory, fn))
            return fn
        if "dist" in diff_files:
            for entry in os.scandir(os.path.join(self.export_directory, "dist")):
                if is_dist_file(entry.name):
                    logging.info("Found tarball %s in dist directory.", entry.name)
                    self.files.append(entry.path)
                    return entry.name
            logging.info("No tarballs found in dist directory.")

        parent_directory = os.path.dirname(self.export_directory)
        diff = set(os.listdir(parent_directory)) - set([self.export_directory])
        if len(diff) == 1:
            fn = diff.pop()
            logging.info("Found tarball %s in parent directory.", fn)
            self.files.append(os.path.join(parent_directory, fn))
            return fn

        if "dist" in new_files:
            for entry in os.scandir(os.path.join(self.export_directory, "dist")):
                if is_dist_file(entry.name) and entry.stat().st_mtime > self.start_time:
                    logging.info("Found tarball %s in dist directory.", entry.name)
                    self.files.append(entry.path)
                    return entry.name

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.find_files()
        return False


def create_dist_schroot(
    tree: Tree,
    target_dir: str,
    chroot: str,
    packaging_tree: Optional[Tree] = None,
    include_controldir: bool = True,
    subdir: Optional[str] = None,
) -> str:
    from .buildsystem import detect_buildsystems
    from .resolver.apt import AptResolver
    from .buildlog import InstallFixer

    if subdir is None:
        subdir = "package"
    with SchrootSession(chroot) as session:
        if packaging_tree is not None:
            from .debian import satisfy_build_deps

            satisfy_build_deps(session, packaging_tree)
        build_dir = os.path.join(session.location, "build")

        try:
            directory = tempfile.mkdtemp(dir=build_dir)
        except OSError as e:
            if e.errno == errno.ENOSPC:
                raise DetailedFailure(1, ["mkdtemp"], NoSpaceOnDevice())
        reldir = "/" + os.path.relpath(directory, session.location)

        export_directory = os.path.join(directory, subdir)
        if not include_controldir:
            export_vcs_tree(tree, export_directory)
        else:
            dupe_vcs_tree(tree, export_directory)

        buildsystems = list(detect_buildsystems(export_directory))
        resolver = AptResolver.from_session(session)
        fixers = [InstallFixer(resolver)]

        with DistCatcher(export_directory) as dc:
            oldcwd = os.getcwd()
            os.chdir(export_directory)
            try:
                session.chdir(os.path.join(reldir, subdir))
                run_dist(session, buildsystems, resolver, fixers)
            finally:
                os.chdir(oldcwd)

        for path in dc.files:
            shutil.copy(path, target_dir)
            return os.path.join(target_dir, os.path.basename(path))

        logging.info("No tarball created :(")
        raise DistNoTarball()


if __name__ == "__main__":
    import argparse
    import breezy.bzr  # noqa: F401
    import breezy.git  # noqa: F401
    from breezy.export import export

    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--chroot",
        default="unstable-amd64-sbuild",
        type=str,
        help="Name of chroot to use",
    )
    parser.add_argument(
        "directory",
        default=".",
        type=str,
        nargs="?",
        help="Directory with upstream source.",
    )
    parser.add_argument(
        "--packaging-directory", type=str, help="Path to packaging directory."
    )
    parser.add_argument(
        "--target-directory", type=str, default="..", help="Target directory"
    )
    parser.add_argument("--verbose", action="store_true", help="Be verbose")

    args = parser.parse_args()

    if args.verbose:
        logging.basicConfig(level=logging.DEBUG, format="%(message)s")
    else:
        logging.basicConfig(level=logging.INFO, format="%(message)s")

    tree = WorkingTree.open(args.directory)
    if args.packaging_directory:
        packaging_tree = WorkingTree.open(args.packaging_directory)
        with packaging_tree.lock_read():
            source = Deb822(packaging_tree.get_file("debian/control"))
        package = source["Source"]
        subdir = package
    else:
        packaging_tree = None
        subdir = None

    try:
        ret = create_dist_schroot(
            tree,
            subdir=subdir,
            target_dir=os.path.abspath(args.target_directory),
            packaging_tree=packaging_tree,
            chroot=args.chroot,
        )
    except NoBuildToolsFound:
        logging.info("No build tools found, falling back to simple export.")
        export(tree, "dist.tar.gz", "tgz", None)
    else:
        print("Created %s" % ret)
    sys.exit(0)

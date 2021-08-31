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

__all__ = [
    "UnidentifiedError",
    "DetailedFailure",
    "create_dist",
    "create_dist_schroot",
]

import errno
import logging
import os
import sys
from typing import Optional, List

from debian.deb822 import Deb822

from breezy.tree import Tree
from breezy.workingtree import WorkingTree

from buildlog_consultant.common import (
    NoSpaceOnDevice,
)


from . import DetailedFailure, UnidentifiedError
from .dist_catcher import DistNoTarball
from .buildsystem import NoBuildToolsFound
from .resolver import auto_resolver
from .session import Session
from .session.schroot import SchrootSession


def run_dist(session, buildsystems, resolver, fixers, target_directory, quiet=False):
    # Some things want to write to the user's home directory,
    # e.g. pip caches in ~/.cache
    session.create_home()

    logging.info('Using dependency resolver: %s', resolver)

    for buildsystem in buildsystems:
        filename = buildsystem.dist(
            session, resolver, fixers, target_directory, quiet=quiet
        )
        return filename

    raise NoBuildToolsFound()


# TODO(jelmer): Remove this function, since it's unused and fairly
# janitor-specific?
def create_dist(
    session: Session,
    tree: Tree,
    target_dir: str,
    include_controldir: bool = True,
    subdir: Optional[str] = None,
    cleanup: bool = False,
) -> Optional[str]:
    if subdir is None:
        subdir = "package"
    try:
        export_directory, reldir = session.setup_from_vcs(
            tree, include_controldir=include_controldir, subdir=subdir
        )
    except OSError as e:
        if e.errno == errno.ENOSPC:
            raise DetailedFailure(1, ["mkdtemp"], NoSpaceOnDevice())
        raise

    return dist(session, export_directory, reldir, target_dir)


def dist(session, export_directory, reldir, target_dir):
    from .fix_build import BuildFixer
    from .buildsystem import detect_buildsystems
    from .buildlog import InstallFixer
    from .fixers import (
        GitIdentityFixer,
        SecretGpgKeyFixer,
        UnexpandedAutoconfMacroFixer,
        GnulibDirectoryFixer,
    )

    # TODO(jelmer): use scan_buildsystems to also look in subdirectories
    buildsystems = list(detect_buildsystems(export_directory))
    resolver = auto_resolver(session)
    fixers: List[BuildFixer] = [
        UnexpandedAutoconfMacroFixer(session, resolver),
        GnulibDirectoryFixer(session)]

    fixers.append(InstallFixer(resolver))

    if session.is_temporary:
        # Only muck about with temporary sessions
        fixers.extend([GitIdentityFixer(session), SecretGpgKeyFixer(session)])

    session.chdir(reldir)
    return run_dist(session, buildsystems, resolver, fixers, target_dir)


def create_dist_schroot(
    tree: Tree,
    target_dir: str,
    chroot: str,
    packaging_tree: Optional[Tree] = None,
    packaging_subpath: Optional[str] = None,
    include_controldir: bool = True,
    subdir: Optional[str] = None,
    cleanup: bool = False,
) -> Optional[str]:
    with SchrootSession(chroot) as session:
        if packaging_tree is not None:
            from .debian import satisfy_build_deps

            satisfy_build_deps(session, packaging_tree, packaging_subpath)
        return create_dist(
            session,
            tree,
            target_dir,
            include_controldir=include_controldir,
            subdir=subdir,
            cleanup=cleanup,
        )


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
    parser.add_argument(
        "--include-controldir", action="store_true", help="Clone rather than export."
    )

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
            include_controldir=args.include_controldir,
        )
    except (NoBuildToolsFound, NotImplementedError):
        logging.info("No build tools found, falling back to simple export.")
        export(tree, "dist.tar.gz", "tgz", None)
    except NotImplementedError:
        logging.info(
            "Build system does not support dist tarball creation, "
            "falling back to simple export."
        )
        export(tree, "dist.tar.gz", "tgz", None)
    except UnidentifiedError as e:
        logging.fatal("Unidentified error: %r", e.lines)
    except DetailedFailure as e:
        logging.fatal("Identified error during dist creation: %s", e.error)
    except DistNoTarball:
        logging.fatal("dist operation did not create a tarball")
    else:
        logging.info("Created %s", ret)
    sys.exit(0)

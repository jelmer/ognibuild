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
    "run_dist",
    "create_dist_schroot",
    "create_dist",
    "dist",
]

import errno
from functools import partial
import logging
import os
import sys
from typing import Optional, List

from breezy.tree import Tree
from breezy.workingtree import WorkingTree

from buildlog_consultant.common import (
    NoSpaceOnDevice,
)

from debian.deb822 import Deb822


from . import DetailedFailure, UnidentifiedError
from .dist_catcher import DistNoTarball
from .fix_build import iterate_with_build_fixers
from .logs import LogManager, NoLogManager
from .buildsystem import NoBuildToolsFound
from .resolver import auto_resolver
from .session import Session
from .session.schroot import SchrootSession


DIST_LOG_FILENAME = 'dist.log'


def run_dist(session, buildsystems, resolver, fixers, target_directory,
             quiet=False, log_manager=None):
    # Some things want to write to the user's home directory,
    # e.g. pip caches in ~/.cache
    session.create_home()

    logging.info('Using dependency resolver: %s', resolver)

    if log_manager is None:
        log_manager = NoLogManager()

    for buildsystem in buildsystems:
        return iterate_with_build_fixers(fixers, log_manager.wrap(
            partial(buildsystem.dist, session, resolver, target_directory,
                    quiet=quiet)))

    raise NoBuildToolsFound()


def dist(session, export_directory, reldir, target_dir, log_manager, *,
         version: Optional[str] = None, quiet=False):
    from .fix_build import BuildFixer
    from .buildsystem import detect_buildsystems
    from .buildlog import InstallFixer
    from .fixers import (
        GitIdentityFixer,
        MissingGoSumEntryFixer,
        SecretGpgKeyFixer,
        UnexpandedAutoconfMacroFixer,
        GnulibDirectoryFixer,
    )

    if version:
        # TODO(jelmer): Shouldn't include backend-specific code here
        os.environ['SETUPTOOLS_SCM_PRETEND_VERSION'] = version

    # TODO(jelmer): use scan_buildsystems to also look in subdirectories
    buildsystems = list(detect_buildsystems(export_directory))
    resolver = auto_resolver(session)
    fixers: List[BuildFixer] = [
        UnexpandedAutoconfMacroFixer(session, resolver),
        GnulibDirectoryFixer(session),
        MissingGoSumEntryFixer(session),
        InstallFixer(resolver)]

    if session.is_temporary:
        # Only muck about with temporary sessions
        fixers.extend([
            GitIdentityFixer(session),
            SecretGpgKeyFixer(session),
        ])

    session.chdir(reldir)

    # Some things want to write to the user's home directory,
    # e.g. pip caches in ~/.cache
    session.create_home()

    logging.info('Using dependency resolver: %s', resolver)

    for buildsystem in buildsystems:
        return iterate_with_build_fixers(fixers, log_manager.wrap(
            partial(
                buildsystem.dist, session, resolver, target_dir,
                quiet=quiet)))

    raise NoBuildToolsFound()


# This is the function used by debianize()
def create_dist(
    session: Session,
    tree: Tree,
    target_dir: str,
    include_controldir: bool = True,
    subdir: Optional[str] = None,
    log_manager: Optional[LogManager] = None,
    version: Optional[str] = None,
) -> Optional[str]:
    """Create a dist tarball for a tree.

    Args:
      session: session to run it
      tree: Tree object to work in
      target_dir: Directory to write tarball into
      include_controldir: Whether to include the version control directory
      subdir: subdirectory in the tree to operate in
    """
    if subdir is None:
        subdir = "package"
    try:
        export_directory, reldir = session.setup_from_vcs(
            tree, include_controldir=include_controldir, subdir=subdir
        )
    except OSError as e:
        if e.errno == errno.ENOSPC:
            raise DetailedFailure(1, ["mkdtemp"], NoSpaceOnDevice()) from e
        raise

    if log_manager is None:
        log_manager = NoLogManager()

    return dist(session, export_directory, reldir, target_dir,
                log_manager=log_manager, version=version)


def create_dist_schroot(
    tree: Tree,
    target_dir: str,
    chroot: str,
    packaging_tree: Optional[Tree] = None,
    packaging_subpath: Optional[str] = None,
    include_controldir: bool = True,
    subdir: Optional[str] = None,
    log_manager: Optional[LogManager] = None,
) -> Optional[str]:
    """Create a dist tarball for a tree.

    Args:
      session: session to run it
      tree: Tree object to work in
      target_dir: Directory to write tarball into
      include_controldir: Whether to include the version control directory
      subdir: subdirectory in the tree to operate in
    """
    with SchrootSession(chroot) as session:
        if packaging_tree is not None:
            from .debian import satisfy_build_deps

            satisfy_build_deps(session, packaging_tree, packaging_subpath)
        return create_dist(
                session, tree, target_dir,
                include_controldir=include_controldir, subdir=subdir,
                log_manager=log_manager)


def main(argv=None):
    import argparse
    import breezy.bzr  # noqa: F401
    import breezy.git  # noqa: F401
    from breezy.export import export

    parser = argparse.ArgumentParser(argv)
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
    parser.add_argument("--mode", choices=["auto", "vcs", "buildsystem"],
                        type=str,
                        help="Mechanism to use to create buildsystem")
    parser.add_argument(
        "--include-controldir", action="store_true",
        help="Clone rather than export."
    )

    args = parser.parse_args()

    if args.verbose:
        logging.basicConfig(level=logging.DEBUG, format="%(message)s")
    else:
        logging.basicConfig(level=logging.INFO, format="%(message)s")

    tree = WorkingTree.open(args.directory)

    packaging_tree: Optional[WorkingTree]
    subdir: Optional[str]

    if args.packaging_directory:
        packaging_tree = WorkingTree.open(args.packaging_directory)
        with packaging_tree.lock_read():  # type: ignore
            source = Deb822(
                packaging_tree.get_file("debian/control"))  # type: ignore
        package = source["Source"]
        subdir = package
    else:
        packaging_tree = None
        subdir = None

    if args.mode == 'vcs':
        export(tree, "dist.tar.gz", "tgz", None)
    elif args.mode in ('auto', 'buildsystem'):
        try:
            ret = create_dist_schroot(
                tree,
                subdir=subdir,
                target_dir=os.path.abspath(args.target_directory),
                packaging_tree=packaging_tree,
                chroot=args.chroot,
                include_controldir=args.include_controldir,
            )
        except NoBuildToolsFound:
            if args.mode == 'buildsystem':
                logging.fatal('No build tools found, unable to create tarball')
                return 1
            logging.info(
                "No build tools found, falling back to simple export.")
            export(tree, "dist.tar.gz", "tgz", None)
        except NotImplementedError:
            if args.mode == 'buildsystem':
                logging.fatal('Unable to ask buildsystem for tarball')
                return 1
            logging.info(
                "Build system does not support dist tarball creation, "
                "falling back to simple export."
            )
            export(tree, "dist.tar.gz", "tgz", None)
        except UnidentifiedError as e:
            logging.fatal("Unidentified error: %r", e.lines)
            return 1
        except DetailedFailure as e:
            logging.fatal("Identified error during dist creation: %s", e.error)
            return 1
        except DistNoTarball:
            logging.fatal("dist operation did not create a tarball")
            return 1
        else:
            logging.info("Created %s", ret)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

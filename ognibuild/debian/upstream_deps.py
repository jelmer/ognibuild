#!/usr/bin/python3
# Copyright (C) 2022 Jelmer Vernooij
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


import logging
import os
import sys
from ognibuild.buildlog import InstallFixer
from ognibuild.resolver.apt import AptResolver

from debmutate.control import ControlEditor, ensure_relation
from debian.deb822 import PkgRelation


def get_project_wide_deps(
        session, wt, subpath, buildsystem, buildsystem_subpath):
    build_deps = []
    test_deps = []

    with session:
        external_dir, internal_dir = session.setup_from_vcs(
            wt, os.path.join(subpath, buildsystem_subpath))

        from ognibuild.debian.udd import popcon_tie_breaker
        from ognibuild.debian.build_deps import BuildDependencyTieBreaker
        apt_resolver = AptResolver.from_session(
            session, tie_breakers=[
                BuildDependencyTieBreaker.from_session(session),
                popcon_tie_breaker,
                ])
        build_fixers = [InstallFixer(apt_resolver)]
        session.chdir(internal_dir)
        try:
            upstream_deps = list(buildsystem.get_declared_dependencies(
                session, build_fixers))
        except NotImplementedError:
            logging.warning('Unable to obtain declared dependencies.')
        else:
            for kind, dep in upstream_deps:
                apt_dep = apt_resolver.resolve(dep)
                if apt_dep is None:
                    logging.warning(
                        'Unable to map upstream requirement %s (kind %s) '
                        'to a Debian package', dep, kind)
                    continue
                logging.debug('Mapped %s (kind: %s) to %s', dep, kind, apt_dep)
                if kind in ('core', 'build'):
                    build_deps.append(apt_dep)
                elif kind in ('core', 'test', ):
                    test_deps.append(apt_dep)
                else:
                    raise ValueError('unknown dependency kind %s' % kind)
    return (build_deps, test_deps)


def main(argv=None):  # noqa: C901
    import argparse

    import breezy
    from breezy.errors import NotBranchError
    from breezy.workingtree import WorkingTree
    breezy.initialize()  # type: ignore
    import breezy.git  # noqa: E402
    import breezy.bzr  # noqa: E402

    from ognibuild.buildsystem import scan_buildsystems
    from ognibuild.session.plain import PlainSession

    parser = argparse.ArgumentParser(prog="deb-sync-upstream-deps")
    parser.add_argument(
        "--verbose", help="be verbose", action="store_true", default=False
    )
    parser.add_argument(
        "--update",
        action="store_true",
        help="Update current package")
    parser.add_argument(
        "--directory", "-d",
        metavar="DIRECTORY",
        help="directory to run in",
        type=str,
        default=".",
    )

    args = parser.parse_args(argv)

    if args.verbose:
        loglevel = logging.DEBUG
    else:
        loglevel = logging.INFO
    logging.basicConfig(level=loglevel, format='%(message)s')

    try:
        wt, subpath = WorkingTree.open_containing(args.directory)
    except NotBranchError as e:
        logging.fatal(
                'please run deps in an existing branch: %s', e)
        return 1

    build_deps = []
    test_deps = []

    session = PlainSession()
    for bs_subpath, bs in scan_buildsystems(wt.abspath(subpath)):
        bs_build_deps, bs_test_deps = get_project_wide_deps(
            session, wt, subpath, bs, bs_subpath)
        build_deps.extend(bs_build_deps)
        test_deps.extend(bs_test_deps)
    if build_deps:
        print('Build-Depends: %s' % ', '.join(
            [x.pkg_relation_str() for x in build_deps]))
    if test_deps:
        print('Test-Depends: %s' % ', '.join(
            [x.pkg_relation_str() for x in test_deps]))
    if args.update:
        with ControlEditor(wt.abspath(
                os.path.join(subpath, 'debian', 'control'))) as control:
            for build_dep in build_deps:
                for rel in build_dep.relations:
                    old_str = control.source.get("Build-Depends", "")
                    new_str = ensure_relation(old_str, PkgRelation.str([rel]))
                    if old_str != new_str:
                        logging.info('Bumped to %s', rel)
                        control.source["Build-Depends"] = new_str


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

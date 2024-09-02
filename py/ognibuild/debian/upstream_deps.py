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

from ognibuild.buildlog import InstallFixer
from ognibuild.resolver.apt import AptResolver


def get_project_wide_deps(
    session, wt, subpath, buildsystem, buildsystem_subpath
):
    build_deps = []
    test_deps = []

    with session:
        external_dir, internal_dir = session.setup_from_vcs(
            wt, os.path.join(subpath, buildsystem_subpath)
        )

        from ognibuild.debian.build_deps import BuildDependencyTieBreaker
        from ognibuild.debian.udd import popcon_tie_breaker

        apt_resolver = AptResolver.from_session(
            session,
            tie_breakers=[
                BuildDependencyTieBreaker.from_session(session),
                popcon_tie_breaker,
            ],
        )
        build_fixers = [InstallFixer(apt_resolver)]
        session.chdir(internal_dir)
        try:
            upstream_deps = list(
                buildsystem.get_declared_dependencies(session, build_fixers)
            )
        except NotImplementedError:
            logging.warning("Unable to obtain declared dependencies.")
        else:
            for kind, dep in upstream_deps:
                apt_dep = apt_resolver.resolve(dep)
                if apt_dep is None:
                    logging.warning(
                        "Unable to map upstream requirement %s (kind %s) "
                        "to a Debian package",
                        dep,
                        kind,
                    )
                    continue
                logging.debug("Mapped %s (kind: %s) to %s", dep, kind, apt_dep)
                if kind in ("core", "build"):
                    build_deps.append(apt_dep)
                elif kind in (
                    "core",
                    "test",
                ):
                    test_deps.append(apt_dep)
                else:
                    raise ValueError(f"unknown dependency kind {kind}")
    return (build_deps, test_deps)

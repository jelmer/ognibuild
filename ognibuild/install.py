#!/usr/bin/python3
# Copyright (C) 2020-2021 Jelmer Vernooij <jelmer@jelmer.uk>
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

from functools import partial
from typing import Optional

from .buildsystem import NoBuildToolsFound, InstallTarget
from .fix_build import iterate_with_build_fixers
from .logs import NoLogManager


def run_install(
        session, buildsystems, resolver, fixers, *, user: bool = False,
        prefix: Optional[str] = None, log_manager=None):
    # Some things want to write to the user's home directory,
    # e.g. pip caches in ~/.cache
    session.create_home()

    if log_manager is None:
        log_manager = NoLogManager()

    install_target = InstallTarget()
    install_target.user = user
    install_target.prefix = prefix

    for buildsystem in buildsystems:
        iterate_with_build_fixers(
            fixers,
            log_manager.wrap(
                partial(buildsystem.install, session, resolver,
                        install_target)))
        return

    raise NoBuildToolsFound()

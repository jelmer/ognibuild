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

import os
from contextlib import suppress
from debian.deb822 import Deb822

from ..session import Session


def satisfy_build_deps(session: Session, tree, debian_path):
    source = Deb822(tree.get_file(os.path.join(debian_path, "control")))
    deps = []
    for name in ["Build-Depends", "Build-Depends-Indep", "Build-Depends-Arch"]:
        with suppress(KeyError):
            deps.append(source[name].strip().strip(","))
    for name in ["Build-Conflicts", "Build-Conflicts-Indep",
                 "Build-Conflicts-Arch"]:
        with suppress(KeyError):
            deps.append("Conflicts: " + source[name])
    deps = [dep.strip().strip(",") for dep in deps]
    from .apt import AptManager

    apt = AptManager(session)
    apt.satisfy(deps)

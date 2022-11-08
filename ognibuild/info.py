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

from typing import Dict, List
from . import Requirement


def run_info(session, buildsystems, fixers=None):
    for buildsystem in buildsystems:
        deps: Dict[str, List[Requirement]] = {}
        try:
            for kind, dep in buildsystem.get_declared_dependencies(
                    session, fixers=fixers):
                deps.setdefault(kind, []).append(dep)
        except NotImplementedError:
            print(
                "\tUnable to detect declared dependencies for this type of "
                "build system"
            )
        if deps:
            print("\tDeclared dependencies:")
            for kind in deps:
                print("\t\t%s:" % kind)
                for dep in deps[kind]:
                    print("\t\t\t%s" % dep)
            print("")
        try:
            outputs = list(buildsystem.get_declared_outputs(
                session, fixers=fixers))
        except NotImplementedError:
            print("\tUnable to detect declared outputs for this type of "
                  "build system")
            outputs = []
        if outputs:
            print("\tDeclared outputs:")
            for output in outputs:
                print("\t\t%s" % output)

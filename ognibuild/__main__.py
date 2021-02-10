# Copyright (C) 2019-2020 Jelmer Vernooij <jelmer@jelmer.uk>
# encoding: utf-8
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
from . import note, UpstreamPackage
from .apt import UnidentifiedError
from .buildsystem import NoBuildToolsFound, detect_buildsystems
from .build import run_build
from .clean import run_clean
from .dist import run_dist
from .install import run_install
from .resolver import (
    AptResolver,
    ExplainResolver,
    AutoResolver,
    NativeResolver,
    MissingDependencies,
)
from .test import run_test


def get_necessary_declared_requirements(resolver, requirements, stages):
    missing = []
    for stage, req in requirements:
        if stage in stages:
            missing.append(req)
    return missing


def install_necessary_declared_requirements(resolver, buildsystem, stages):
    missing = []
    missing.extend(
        get_necessary_declared_requirements(
            resolver, buildsystem.get_declared_dependencies(), stages
        )
    )
    resolver.install(missing)


STAGE_MAP = {
    "dist": [],
    "install": ["build"],
    "test": ["test", "dev"],
    "build": ["build"],
    "clean": [],
}


def main():  # noqa: C901
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument(
        "subcommand", type=str, choices=["dist", "build", "clean", "test", "install"]
    )
    parser.add_argument(
        "--directory", "-d", type=str, help="Directory for project.", default="."
    )
    parser.add_argument("--schroot", type=str, help="schroot to run in.")
    parser.add_argument(
        "--resolve",
        choices=["explain", "apt", "native"],
        default="apt",
        help="What to do about missing dependencies",
    )
    parser.add_argument(
        "--ignore-declared-dependencies",
        action="store_true",
        help="Ignore declared dependencies, follow build errors only",
    )
    args = parser.parse_args()
    if args.schroot:
        from .session.schroot import SchrootSession

        session = SchrootSession(args.schroot)
    else:
        from .session.plain import PlainSession

        session = PlainSession()
    with session:
        if args.resolve == "apt":
            resolver = AptResolver.from_session(session)
        elif args.resolve == "explain":
            resolver = ExplainResolver.from_session(session)
        elif args.resolve == "native":
            resolver = NativeResolver.from_session(session)
        elif args.resolver == "auto":
            resolver = AutoResolver.from_session(session)
        os.chdir(args.directory)
        try:
            bss = list(detect_buildsystems(args.directory))
            if not args.ignore_declared_dependencies:
                stages = STAGE_MAP[args.subcommand]
                if stages:
                    for bs in bss:
                        install_necessary_declared_requirements(resolver, bs, stages)
            if args.subcommand == "dist":
                run_dist(session=session, buildsystems=bss, resolver=resolver)
            if args.subcommand == "build":
                run_build(session, buildsystems=bss, resolver=resolver)
            if args.subcommand == "clean":
                run_clean(session, buildsystems=bss, resolver=resolver)
            if args.subcommand == "install":
                run_install(session, buildsystems=bss, resolver=resolver)
            if args.subcommand == "test":
                run_test(session, buildsystems=bss, resolver=resolver)
        except UnidentifiedError:
            return 1
        except NoBuildToolsFound:
            logging.info("No build tools found.")
            return 1
        except MissingDependencies as e:
            for req in e.requirements:
                note("Missing dependency (%s:%s)" % (req.family, req.name))
                for resolver in [
                    AptResolver.from_session(session),
                    NativeResolver.from_session(session),
                ]:
                    note("  %s" % (resolver.explain([req]),))
            return 2
        return 0


sys.exit(main())

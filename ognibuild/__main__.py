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
import shlex
import sys
from . import UnidentifiedError, DetailedFailure
from .buildlog import InstallFixer, ExplainInstallFixer, ExplainInstall
from .buildsystem import NoBuildToolsFound, detect_buildsystems
from .resolver import (
    auto_resolver,
    native_resolvers,
    UnsatisfiedRequirements,
)
from .resolver.apt import AptResolver


def display_explain_commands(commands):
    logging.info("Run one or more of the following commands:")
    for command, reqs in commands:
        if isinstance(command, list):
            command = shlex.join(command)
        logging.info(
            '  %s (to install %s)', command, ', '.join(map(str, reqs)))


def get_necessary_declared_requirements(resolver, requirements, stages):
    missing = []
    for stage, req in requirements:
        if stage in stages:
            missing.append(req)
    return missing


def install_necessary_declared_requirements(session, resolver, buildsystems, stages, explain=False):
    relevant = []
    declared_reqs = []
    for buildsystem in buildsystems:
        try:
            declared_reqs.extend(buildsystem.get_declared_dependencies())
        except NotImplementedError:
            logging.warning(
                "Unable to determine declared dependencies from %r", buildsystem
            )
    relevant.extend(
        get_necessary_declared_requirements(resolver, declared_reqs, stages)
    )
    missing = []
    for req in relevant:
        try:
            if not req.met(session):
                missing.append(req)
        except NotImplementedError:
            missing.append(req)
    if missing:
        if explain:
            commands = resolver.explain(missing)
            if not commands:
                raise UnsatisfiedRequirements(missing)
            raise ExplainInstall(commands)
        else:
            resolver.install(missing)


# Types of dependencies:
# - core: necessary to do anything with the package
# - build: necessary to build the package
# - test: necessary to run the tests
# - dev: necessary for development (e.g. linters, yacc)

STAGE_MAP = {
    "dist": [],
    "info": [],
    "install": ["core", "build"],
    "test": ["test", "build", "core"],
    "build": ["build", "core"],
    "clean": [],
}


def determine_fixers(session, resolver, explain=False):
    if explain:
        return [ExplainInstallFixer(resolver)]
    else:
        return [InstallFixer(resolver)]


def main():  # noqa: C901
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--directory", "-d", type=str, help="Directory for project.", default="."
    )
    parser.add_argument("--schroot", type=str, help="schroot to run in.")
    parser.add_argument(
        "--resolve",
        choices=["apt", "native", "auto"],
        default="auto",
        help="What to do about missing dependencies",
    )
    parser.add_argument(
        "--explain",
        action="store_true",
        help="Explain what needs to be done rather than making changes",
    )
    parser.add_argument(
        "--ignore-declared-dependencies",
        "--optimistic",
        action="store_true",
        help="Ignore declared dependencies, follow build errors only",
    )
    parser.add_argument("--verbose", action="store_true", help="Be verbose")
    subparsers = parser.add_subparsers(dest="subcommand")
    subparsers.add_parser("dist")
    subparsers.add_parser("build")
    subparsers.add_parser("clean")
    subparsers.add_parser("test")
    subparsers.add_parser("info")
    install_parser = subparsers.add_parser("install")
    install_parser.add_argument(
        "--user", action="store_true", help="Install in local-user directories."
    )

    args = parser.parse_args()
    if not args.subcommand:
        parser.print_usage()
        return 1
    if args.verbose:
        logging.basicConfig(level=logging.DEBUG, format="%(message)s")
    else:
        logging.basicConfig(level=logging.INFO, format="%(message)s")
    if args.schroot:
        from .session.schroot import SchrootSession

        session = SchrootSession(args.schroot)
    else:
        from .session.plain import PlainSession

        session = PlainSession()
    with session:
        if args.resolve == "apt":
            resolver = AptResolver.from_session(session)
        elif args.resolve == "native":
            resolver = native_resolvers(session)
        elif args.resolve == "auto":
            resolver = auto_resolver(session)
        logging.info("Using requirement resolver: %s", resolver)
        os.chdir(args.directory)
        try:
            bss = list(detect_buildsystems(args.directory))
            logging.info(
                "Detected buildsystems: %s", ', '.join(map(str, bss)))
            if not args.ignore_declared_dependencies:
                stages = STAGE_MAP[args.subcommand]
                if stages:
                    logging.info("Checking that declared requirements are present")
                    try:
                        install_necessary_declared_requirements(
                            session, resolver, bss, stages, explain=args.explain)
                    except ExplainInstall as e:
                        display_explain_commands(e.commands)
                        return 1
            fixers = determine_fixers(session, resolver, explain=args.explain)
            if args.subcommand == "dist":
                from .dist import run_dist

                run_dist(
                    session=session, buildsystems=bss, resolver=resolver, fixers=fixers
                )
            if args.subcommand == "build":
                from .build import run_build

                run_build(session, buildsystems=bss, resolver=resolver, fixers=fixers)
            if args.subcommand == "clean":
                from .clean import run_clean

                run_clean(session, buildsystems=bss, resolver=resolver, fixers=fixers)
            if args.subcommand == "install":
                from .install import run_install

                run_install(
                    session,
                    buildsystems=bss,
                    resolver=resolver,
                    fixers=fixers,
                    user=args.user,
                )
            if args.subcommand == "test":
                from .test import run_test

                run_test(session, buildsystems=bss, resolver=resolver, fixers=fixers)
            if args.subcommand == "info":
                from .info import run_info

                run_info(session, buildsystems=bss)
        except ExplainInstall as e:
            display_explain_commands(e.commands)
        except (UnidentifiedError, DetailedFailure):
            return 1
        except NoBuildToolsFound:
            logging.info("No build tools found.")
            return 1
        return 0


sys.exit(main())

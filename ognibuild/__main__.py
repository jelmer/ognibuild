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
from . import UnidentifiedError
from .buildsystem import NoBuildToolsFound, detect_buildsystems
from .resolver import (
    ExplainResolver,
    AutoResolver,
    native_resolvers,
    MissingDependencies,
)
from .resolver.apt import AptResolver


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
    "info": [],
    "install": ["build"],
    "test": ["test", "dev"],
    "build": ["build"],
    "clean": [],
}


def main():  # noqa: C901
    import argparse

    parser = argparse.ArgumentParser()
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
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Be verbose")
    subparsers = parser.add_subparsers(dest='subcommand')
    subparsers.add_parser('dist')
    subparsers.add_parser('build')
    subparsers.add_parser('clean')
    subparsers.add_parser('test')
    subparsers.add_parser('info')
    install_parser = subparsers.add_parser('install')
    install_parser.add_argument(
        '--user', action='store_true', help='Install in local-user directories.')

    args = parser.parse_args()
    if not args.subcommand:
        parser.print_usage()
        return 1
    if args.verbose:
        logging.basicConfig(level=logging.DEBUG)
    else:
        logging.basicConfig(level=logging.INFO)
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
            resolver = native_resolvers(session)
        elif args.resolver == "auto":
            resolver = AutoResolver.from_session(session)
        os.chdir(args.directory)
        try:
            bss = list(detect_buildsystems(args.directory))
            logging.info('Detected buildsystems: %r', bss)
            if not args.ignore_declared_dependencies:
                stages = STAGE_MAP[args.subcommand]
                if stages:
                    for bs in bss:
                        install_necessary_declared_requirements(resolver, bs, stages)
            if args.subcommand == "dist":
                from .dist import run_dist
                run_dist(session=session, buildsystems=bss, resolver=resolver)
            if args.subcommand == "build":
                from .build import run_build
                run_build(session, buildsystems=bss, resolver=resolver)
            if args.subcommand == "clean":
                from .clean import run_clean
                run_clean(session, buildsystems=bss, resolver=resolver)
            if args.subcommand == "install":
                from .install import run_install
                run_install(
                    session, buildsystems=bss, resolver=resolver,
                    user=args.user)
            if args.subcommand == "test":
                from .test import run_test
                run_test(session, buildsystems=bss, resolver=resolver)
            if args.subcommand == "info":
                from .info import run_info
                run_info(session, buildsystems=bss, resolver=resolver)
        except UnidentifiedError:
            return 1
        except NoBuildToolsFound:
            logging.info("No build tools found.")
            return 1
        except MissingDependencies as e:
            for req in e.requirements:
                logging.info("Missing dependency (%s:%s)",
                             req.family, req.package)
                for resolver in [
                    AptResolver.from_session(session),
                    native_resolvers(session),
                ]:
                    logging.info("  %s", resolver.explain([req]))
            return 2
        return 0


sys.exit(main())

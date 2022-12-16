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

from contextlib import ExitStack
import logging
import os
import shlex
import sys
import tempfile
from urllib.parse import urlparse
from . import UnidentifiedError, DetailedFailure, version_string
from .buildlog import (
    InstallFixer,
    ExplainInstallFixer,
    ExplainInstall,
    install_missing_reqs,
)
from .buildsystem import NoBuildToolsFound, detect_buildsystems
from .resolver import (
    auto_resolver,
    select_resolvers,
    UnsatisfiedRequirements,
)
from .session import SessionSetupFailure


def display_explain_commands(commands):
    logging.info("Run one or more of the following commands:")
    for command, reqs in commands:
        if isinstance(command, list):
            command = shlex.join(command)  # type: ignore
        logging.info(
            "  %s (to install %s)", command, ", ".join(map(str, reqs)))


def install_necessary_declared_requirements(
    session, resolver, fixers, buildsystems, stages, explain=False
):

    if explain:
        relevant = []
        for buildsystem in buildsystems:
            declared_reqs = buildsystem.get_declared_dependencies(
                session, fixers)
            for stage, req in declared_reqs:
                if stage in stages:
                    relevant.append(req)
        install_missing_reqs(session, resolver, relevant, explain=True)
    else:
        for buildsystem in buildsystems:
            try:
                buildsystem.install_declared_requirements(
                    stages, session, resolver, fixers)
            except NotImplementedError:
                logging.warning(
                    "Unable to determine declared dependencies from %r",
                    buildsystem
                )


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
    "verify": ["build", "core", "test"],
}


def determine_fixers(session, resolver, explain=False):
    if explain:
        return [ExplainInstallFixer(resolver)]
    else:
        return [InstallFixer(resolver)]


def main():  # noqa: C901
    import argparse
    from .session import Session

    parser = argparse.ArgumentParser(prog='ogni')
    parser.add_argument(
        "--version", action="version", version="%(prog)s " + version_string
    )
    parser.add_argument(
        "--directory", "-d", type=str, help="Directory for project.",
        default="."
    )
    parser.add_argument("--schroot", type=str, help="schroot to run in.")
    parser.add_argument(
        "--resolve",
        choices=["apt", "native", "auto"],
        default="auto",
        help="What to do about missing dependencies",
    )
    parser.add_argument(
        '--apt', help=argparse.SUPPRESS,
        dest='resolve', action='store_const', const='apt')
    parser.add_argument(
        '--native', help=argparse.SUPPRESS,
        dest='native', action='store_const', const='native')
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
    parser.add_argument(
        "--user", action="store_true",
        help="Install in local-user directories."
    )
    parser.add_argument(
        "--dep-server-url", type=str,
        help="ognibuild dep server to use",
        default=os.environ.get('OGNIBUILD_DEPS'))

    parser.add_argument("--verbose", action="store_true", help="Be verbose")
    subparsers = parser.add_subparsers(dest="subcommand")
    subparsers.add_parser("dist")
    subparsers.add_parser("build")
    subparsers.add_parser("clean")
    subparsers.add_parser("test")
    subparsers.add_parser("info")
    subparsers.add_parser("verify")
    exec_parser = subparsers.add_parser("exec")
    exec_parser.add_argument(
        'subargv', nargs=argparse.REMAINDER, help='Command to run.')
    install_parser = subparsers.add_parser("install")
    install_parser.add_argument(
        "--prefix", type=str, help='Prefix to install in')

    args = parser.parse_args()
    if not args.subcommand:
        parser.print_usage()
        return 1
    if args.verbose:
        logging.basicConfig(level=logging.DEBUG, format="%(message)s")
    else:
        logging.basicConfig(level=logging.INFO, format="%(message)s")

    session: Session
    if args.schroot:
        from .session.schroot import SchrootSession

        session = SchrootSession(args.schroot)
    else:
        from .session.plain import PlainSession

        session = PlainSession()

    with ExitStack() as es:
        try:
            es.enter_context(session)
        except SessionSetupFailure as e:
            logging.debug('Error lines: %r', e.errlines)
            logging.fatal('Failed to set up session: %s', e.reason)
            return 1

        parsed_url = urlparse(args.directory)
        # TODO(jelmer): Get a list of supported schemes from breezy?
        if parsed_url.scheme in ('git', 'http', 'https', 'ssh'):
            import breezy.git  # noqa: F401
            import breezy.bzr  # noqa: F401
            from breezy.branch import Branch
            b = Branch.open(args.directory)
            logging.info("Cloning %s", args.directory)
            td = es.enter_context(tempfile.TemporaryDirectory())
            to_dir = b.controldir.sprout(td, create_tree_if_local=True)
            external_dir, internal_dir = session.setup_from_vcs(
                to_dir.open_workingtree())
        else:
            if parsed_url.scheme == 'file':
                directory = parsed_url.path
            else:
                directory = args.directory
            logging.info("Preparing directory %s", directory)
            external_dir, internal_dir = session.setup_from_directory(
                directory)
        session.chdir(internal_dir)
        os.chdir(external_dir)

        if not session.is_temporary and args.subcommand == 'info':
            args.explain = True

        if args.resolve == "auto":
            resolver = auto_resolver(session, explain=args.explain)
        else:
            resolver = select_resolvers(
                session, user_local=args.user,
                resolvers=args.resolve.split(','),
                dep_server_url=args.dep_server_url)
        logging.info("Using requirement resolver: %s", resolver)
        fixers = determine_fixers(session, resolver, explain=args.explain)
        try:
            if args.subcommand == "exec":
                from .fix_build import run_with_build_fixers
                run_with_build_fixers(fixers, session, args.subargv)
                return 0
            bss = list(detect_buildsystems(external_dir))
            logging.info("Detected buildsystems: %s", ", ".join(map(str, bss)))
            if not args.ignore_declared_dependencies:
                stages = STAGE_MAP[args.subcommand]
                if stages:
                    logging.info(
                        "Checking that declared requirements are present")
                    try:
                        install_necessary_declared_requirements(
                            session, resolver, fixers, bss, stages,
                            explain=args.explain
                        )
                    except UnsatisfiedRequirements as e:
                        logging.info(
                            'Unable to install declared dependencies:')
                        for req in e.requirements:
                            logging.info(' * %s', req)
                        return 1
                    except ExplainInstall as e:
                        display_explain_commands(e.commands)
                        return 1
            if args.subcommand == "dist":
                from .dist import run_dist, DistNoTarball

                try:
                    run_dist(
                        session=session,
                        buildsystems=bss,
                        resolver=resolver,
                        fixers=fixers,
                        target_directory=".",
                    )
                except DistNoTarball:
                    logging.fatal('No tarball created.')
                    return 1
            if args.subcommand == "build":
                from .build import run_build

                run_build(
                    session, buildsystems=bss, resolver=resolver,
                    fixers=fixers)
            if args.subcommand == "clean":
                from .clean import run_clean

                run_clean(
                    session, buildsystems=bss, resolver=resolver,
                    fixers=fixers)
            if args.subcommand == "install":
                from .install import run_install

                run_install(
                    session,
                    buildsystems=bss,
                    resolver=resolver,
                    fixers=fixers,
                    user=args.user,
                    prefix=args.prefix,
                )
            if args.subcommand == "test":
                from .test import run_test

                run_test(
                    session, buildsystems=bss, resolver=resolver,
                    fixers=fixers)
            if args.subcommand == "info":
                from .info import run_info

                run_info(session, buildsystems=bss, fixers=fixers)

            if args.subcommand == "verify":
                from .build import run_build
                from .test import run_test

                run_build(
                    session, buildsystems=bss, resolver=resolver,
                    fixers=fixers)
                run_test(
                    session, buildsystems=bss, resolver=resolver,
                    fixers=fixers)

        except ExplainInstall as e:
            display_explain_commands(e.commands)
        except UnidentifiedError:
            logging.info(
                'If there is a clear indication of a problem in the build '
                'log, please consider filing a request to update the patterns '
                'in buildlog-consultant at '
                'https://github.com/jelmer/buildlog-consultant/issues/new')
            return 1
        except DetailedFailure:
            if not args.verbose:
                logging.info(
                    'Run with --verbose to get more information '
                    'about steps taken to try to resolve error')
            logging.info(
                'Please consider filing a bug report at '
                'https://github.com/jelmer/ognibuild/issues/new')
            return 1
        except NoBuildToolsFound:
            logging.info("No build tools found.")
            return 1
        return 0


sys.exit(main())

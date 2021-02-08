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
from .buildsystem import NoBuildToolsFound
from .build import run_build
from .clean import run_clean
from .dist import run_dist
from .install import run_install
from .test import run_test


def main():
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
        '--resolve', choices=['explain', 'apt', 'native'],
        default='apt',
        help='What to do about missing dependencies')
    args = parser.parse_args()
    if args.schroot:
        from .session.schroot import SchrootSession

        session = SchrootSession(args.schroot)
    else:
        from .session.plain import PlainSession

        session = PlainSession()
    with session:
        if args.resolve == 'apt':
            from .resolver import AptResolver
            resolver = AptResolver.from_session(session)
        elif args.resolve == 'explain':
            from .resolver import ExplainResolver
            resolver = ExplainResolver.from_session(session)
        elif args.resolve == 'native':
            from .resolver import NativeResolver
            resolver = NativeResolver.from_session(session)
        os.chdir(args.directory)
        try:
            if args.subcommand == 'dist':
                run_dist(session=session, resolver=resolver)
            if args.subcommand == 'build':
                run_build(session, resolver=resolver)
            if args.subcommand == 'clean':
                run_clean(session, resolver=resolver)
            if args.subcommand == 'install':
                run_install(session, resolver=resolver)
            if args.subcommand == 'test':
                run_test(session, resolver=resolver)
        except NoBuildToolsFound:
            logging.info("No build tools found.")
            return 1
        return 0


sys.exit(main())

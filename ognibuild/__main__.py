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

import os
import sys
from . import (
    run_dist, run_build, run_clean, run_install, run_test, NoBuildToolsFound,
    note
    )


def main():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument(
        'subcommand', type=str,
        choices=['dist', 'build', 'clean', 'test', 'install'])
    parser.add_argument(
        '--directory', '-d', type=str, help='Directory for project.',
        default='.')
    parser.add_argument(
        '--schroot', type=str, help='schroot to run in.')
    args = parser.parse_args()
    if args.schroot:
        from .session.schroot import SchrootSession
        session = SchrootSession(args.schroot)
    else:
        from .session.plain import PlainSession
        session = PlainSession()
    with session:
        os.chdir(args.directory)
        try:
            if args.subcommand == 'dist':
                run_dist(session)
            if args.subcommand == 'build':
                run_build(session)
            if args.subcommand == 'clean':
                run_clean(session)
            if args.subcommand == 'install':
                run_install(session)
            if args.subcommand == 'test':
                run_test(session)
        except NoBuildToolsFound:
            note('No build tools found.')
            return 1
        return 0


sys.exit(main())

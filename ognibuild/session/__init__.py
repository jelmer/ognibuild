#!/usr/bin/python
# Copyright (C) 2020 Jelmer Vernooij <jelmer@jelmer.uk>
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

from typing import Optional, List, Dict
import sys
import subprocess


class Session(object):
    def __enter__(self) -> "Session":
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        return False

    def chdir(self, cwd: str) -> None:
        raise NotImplementedError(self.chdir)

    @property
    def location(self) -> str:
        raise NotImplementedError

    def check_call(
        self,
        argv: List[str],
        cwd: Optional[str] = None,
        user: Optional[str] = None,
        env: Optional[Dict[str, str]] = None,
    ):
        raise NotImplementedError(self.check_call)

    def check_output(
        self,
        argv: List[str],
        cwd: Optional[str] = None,
        user: Optional[str] = None,
        env: Optional[Dict[str, str]] = None,
    ) -> bytes:
        raise NotImplementedError(self.check_output)

    def Popen(
        self, argv, cwd: Optional[str] = None, user: Optional[str] = None, **kwargs
    ):
        raise NotImplementedError(self.Popen)

    def call(
        self, argv: List[str], cwd: Optional[str] = None, user: Optional[str] = None
    ):
        raise NotImplementedError(self.call)

    def create_home(self) -> None:
        """Create the user's home directory."""
        raise NotImplementedError(self.create_home)

    def exists(self, path: str) -> bool:
        """Check whether a path exists in the chroot."""
        raise NotImplementedError(self.exists)

    def scandir(self, path: str):
        raise NotImplementedError(self.scandir)


class SessionSetupFailure(Exception):
    """Session failed to be set up."""


def run_with_tee(session: Session, args: List[str], **kwargs):
    p = session.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, **kwargs)
    contents = []
    while p.poll() is None:
        line = p.stdout.readline()
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
        contents.append(line.decode("utf-8", "surrogateescape"))
    return p.returncode, contents

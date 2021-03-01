#!/usr/bin/python3
# Copyright (C) 2020 Jelmer Vernooij <jelmer@jelmer.uk>
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
import subprocess

from typing import Optional, List, Dict


from . import Session, SessionSetupFailure


class SchrootSession(Session):

    _cwd: Optional[str]
    _location: Optional[str]
    chroot: str

    def __init__(self, chroot: str):
        if not isinstance(chroot, str):
            raise TypeError("not a valid chroot: %r" % chroot)
        self.chroot = chroot
        self._location = None
        self._cwd = None

    def _get_location(self) -> str:
        return (
            subprocess.check_output(
                ["schroot", "--location", "-c", "session:" + self.session_id]
            )
            .strip()
            .decode()
        )

    def _end_session(self) -> None:
        subprocess.check_output(["schroot", "-c", "session:" + self.session_id, "-e"])

    def __enter__(self) -> "Session":
        try:
            self.session_id = (
                subprocess.check_output(["schroot", "-c", self.chroot, "-b"])
                .strip()
                .decode()
            )
        except subprocess.CalledProcessError:
            # TODO(jelmer): Capture stderr and forward in SessionSetupFailure
            raise SessionSetupFailure()
        logging.info(
            "Opened schroot session %s (from %s)", self.session_id, self.chroot
        )
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self._end_session()
        return False

    def chdir(self, cwd: str) -> None:
        self._cwd = cwd

    @property
    def location(self) -> str:
        if self._location is None:
            self._location = self._get_location()
        return self._location

    def _run_argv(
        self,
        argv: List[str],
        cwd: Optional[str] = None,
        user: Optional[str] = None,
        env: Optional[Dict[str, str]] = None,
    ):
        base_argv = ["schroot", "-r", "-c", "session:" + self.session_id]
        if cwd is None:
            cwd = self._cwd
        if cwd is not None:
            base_argv.extend(["-d", cwd])
        if user is not None:
            base_argv.extend(["-u", user])
        if env:
            argv = [
                "sh",
                "-c",
                " ".join(
                    [
                        "%s=%s " % (key, shlex.quote(value))
                        for (key, value) in env.items()
                    ]
                    + [shlex.quote(arg) for arg in argv]
                ),
            ]
        return base_argv + ["--"] + argv

    def check_call(
        self,
        argv: List[str],
        cwd: Optional[str] = None,
        user: Optional[str] = None,
        env: Optional[Dict[str, str]] = None,
    ):
        try:
            subprocess.check_call(self._run_argv(argv, cwd, user, env=env))
        except subprocess.CalledProcessError as e:
            raise subprocess.CalledProcessError(e.returncode, argv)

    def check_output(
        self,
        argv: List[str],
        cwd: Optional[str] = None,
        user: Optional[str] = None,
        env: Optional[Dict[str, str]] = None,
    ) -> bytes:
        try:
            return subprocess.check_output(self._run_argv(argv, cwd, user, env=env))
        except subprocess.CalledProcessError as e:
            raise subprocess.CalledProcessError(e.returncode, argv)

    def Popen(
        self, argv, cwd: Optional[str] = None, user: Optional[str] = None, **kwargs
    ):
        return subprocess.Popen(self._run_argv(argv, cwd, user), **kwargs)

    def call(
        self, argv: List[str], cwd: Optional[str] = None, user: Optional[str] = None
    ):
        return subprocess.call(self._run_argv(argv, cwd, user))

    def create_home(self) -> None:
        """Create the user's home directory."""
        home = (
            self.check_output(["sh", "-c", "echo $HOME"], cwd="/").decode().rstrip("\n")
        )
        user = (
            self.check_output(["sh", "-c", "echo $LOGNAME"], cwd="/")
            .decode()
            .rstrip("\n")
        )
        logging.info("Creating directory %s", home)
        self.check_call(["mkdir", "-p", home], cwd="/", user="root")
        self.check_call(["chown", user, home], cwd="/", user="root")

    def _fullpath(self, path: str) -> str:
        if self._cwd is None:
            raise ValueError("no cwd set")
        return os.path.join(self.location, os.path.join(self._cwd, path).lstrip("/"))

    def exists(self, path: str) -> bool:
        fullpath = self._fullpath(path)
        return os.path.exists(fullpath)

    def scandir(self, path: str):
        fullpath = self._fullpath(path)
        return os.scandir(fullpath)

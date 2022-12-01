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
import tempfile

from typing import Optional, List, Dict


from . import Session, SessionSetupFailure, NoSessionOpen, SessionAlreadyOpen


class SchrootSession(Session):

    _cwd: Optional[str]
    _location: Optional[str]
    chroot: str
    session_id: Optional[str]

    def __init__(self, chroot: str):
        if not isinstance(chroot, str):
            raise TypeError("not a valid chroot: %r" % chroot)
        self.chroot = chroot
        self._location = None
        self._cwd = None
        self.session_id = None

    def _get_location(self) -> str:
        if self.session_id is None:
            raise NoSessionOpen(self)
        return (
            subprocess.check_output(
                ["schroot", "--location", "-c", "session:" + self.session_id]
            )
            .strip()
            .decode()
        )

    def _end_session(self) -> bool:
        if self.session_id is None:
            raise NoSessionOpen(self)
        try:
            subprocess.check_output(
                ["schroot", "-c", "session:" + self.session_id, "-e"],
                stderr=subprocess.PIPE,
            )
        except subprocess.CalledProcessError as e:
            for line in e.stderr.splitlines(False):
                if line.startswith(b"E: "):
                    logging.error("%s", line[3:].decode(errors="replace"))
            logging.warning(
                "Failed to close schroot session %s, leaving stray.",
                self.session_id
            )
            self.session_id = None
            return False
        self.session_id = None
        self._location = None
        return True

    def __enter__(self) -> "Session":
        if self.session_id is not None:
            raise SessionAlreadyOpen(self)
        stderr = tempfile.TemporaryFile()
        try:
            self.session_id = (
                subprocess.check_output(
                    ["schroot", "-c", self.chroot, "-b"], stderr=stderr)
                .strip()
                .decode()
            )
        except subprocess.CalledProcessError as e:
            stderr.seek(0)
            errlines = stderr.readlines()
            if len(errlines) == 1:
                raise SessionSetupFailure(
                    errlines[0].rstrip().decode(), errlines=errlines) from e
            elif len(errlines) == 0:
                raise SessionSetupFailure(
                    "No output from schroot", errlines=errlines) from e
            else:
                raise SessionSetupFailure(
                    errlines[-1].decode(), errlines=errlines) from e
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
        if self.session_id is None:
            raise NoSessionOpen(self)
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
        close_fds: bool = True,
    ):
        try:
            subprocess.check_call(
                self._run_argv(argv, cwd, user, env=env), close_fds=close_fds
            )
        except subprocess.CalledProcessError as e:
            raise subprocess.CalledProcessError(e.returncode, argv) from e

    def check_output(
        self,
        argv: List[str],
        cwd: Optional[str] = None,
        user: Optional[str] = None,
        env: Optional[Dict[str, str]] = None,
    ) -> bytes:
        try:
            return subprocess.check_output(
                self._run_argv(argv, cwd, user, env=env))
        except subprocess.CalledProcessError as e:
            raise subprocess.CalledProcessError(e.returncode, argv) from e

    def Popen(
        self, argv, cwd: Optional[str] = None, user: Optional[str] = None,
        **kwargs
    ):
        return subprocess.Popen(self._run_argv(argv, cwd, user), **kwargs)

    def call(
        self, argv: List[str], cwd: Optional[str] = None,
        user: Optional[str] = None
    ):
        return subprocess.call(self._run_argv(argv, cwd, user))

    def create_home(self) -> None:
        """Create the user's home directory."""
        home = (
            self.check_output(
                ["sh", "-c", "echo $HOME"], cwd="/").decode().rstrip("\n")
        )
        user = (
            self.check_output(["sh", "-c", "echo $LOGNAME"], cwd="/")
            .decode()
            .rstrip("\n")
        )
        logging.info("Creating directory %s in schroot session.", home)
        self.check_call(["mkdir", "-p", home], cwd="/", user="root")
        self.check_call(["chown", user, home], cwd="/", user="root")

    def external_path(self, path: str) -> str:
        if os.path.isabs(path):
            return os.path.join(self.location, path.lstrip("/"))
        if self._cwd is None:
            raise ValueError("no cwd set")
        return os.path.join(
            self.location, os.path.join(self._cwd, path).lstrip("/"))

    def exists(self, path: str) -> bool:
        fullpath = self.external_path(path)
        return os.path.exists(fullpath)

    def scandir(self, path: str):
        fullpath = self.external_path(path)
        return os.scandir(fullpath)

    def mkdir(self, path: str):
        fullpath = self.external_path(path)
        return os.mkdir(fullpath)

    def rmtree(self, path: str):
        import shutil
        fullpath = self.external_path(path)
        return shutil.rmtree(fullpath)

    def setup_from_vcs(
        self, tree, include_controldir: Optional[bool] = None, subdir="package"
    ):
        from ..vcs import dupe_vcs_tree, export_vcs_tree

        build_dir = os.path.join(self.location, "build")
        directory = tempfile.mkdtemp(dir=build_dir)
        reldir = "/" + os.path.relpath(directory, self.location)

        export_directory = os.path.join(directory, subdir)
        if not include_controldir:
            export_vcs_tree(tree, export_directory)
        else:
            dupe_vcs_tree(tree, export_directory)

        return export_directory, os.path.join(reldir, subdir)

    def setup_from_directory(self, path, subdir="package"):
        import shutil

        build_dir = os.path.join(self.location, "build")
        directory = tempfile.mkdtemp(dir=build_dir)
        reldir = "/" + os.path.relpath(directory, self.location)
        export_directory = os.path.join(directory, subdir)
        shutil.copytree(path, export_directory, symlinks=True)
        return export_directory, os.path.join(reldir, subdir)

    is_temporary = True

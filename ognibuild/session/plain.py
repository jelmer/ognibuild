#!/usr/bin/python
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

from . import Session, NoSessionOpen, SessionAlreadyOpen

import contextlib
import os
import shutil
import subprocess
import tempfile
from typing import Optional, Dict, List


class PlainSession(Session):
    """Session ignoring user."""

    location = "/"

    def __init__(self):
        self.es = None

    def _prepend_user(self, user, args):
        if self.es is None:
            raise NoSessionOpen(self)
        if user is not None:
            import getpass

            if user != getpass.getuser():
                args = ["sudo", "-u", user] + args
        return args

    def __repr__(self):
        return "%s()" % (type(self).__name__,)

    def __enter__(self) -> "Session":
        if self.es is not None:
            raise SessionAlreadyOpen(self)
        self.es = contextlib.ExitStack()
        self.es.__enter__()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        if self.es is None:
            raise NoSessionOpen(self)
        self.es.__exit__(exc_type, exc_val, exc_tb)
        self.es = None
        return False

    def create_home(self):
        pass

    def check_call(
        self,
        argv: List[str],
        cwd: Optional[str] = None,
        user: Optional[str] = None,
        env: Optional[Dict[str, str]] = None,
        close_fds: bool = True,
    ):
        argv = self._prepend_user(user, argv)
        return subprocess.check_call(
            argv, cwd=cwd, env=env, close_fds=close_fds)

    def check_output(
        self,
        argv: List[str],
        cwd: Optional[str] = None,
        user: Optional[str] = None,
        env: Optional[Dict[str, str]] = None,
    ) -> bytes:
        argv = self._prepend_user(user, argv)
        return subprocess.check_output(argv, cwd=cwd, env=env)

    def Popen(
            self, args, stdout=None, stderr=None, stdin=None, user=None,
            cwd=None, env=None):
        args = self._prepend_user(user, args)
        return subprocess.Popen(
            args, stdout=stdout, stderr=stderr, stdin=stdin, cwd=cwd, env=env)

    def exists(self, path):
        return os.path.exists(path)

    def rmtree(self, path):
        return shutil.rmtree(path)

    def scandir(self, path):
        return os.scandir(path)

    def chdir(self, path):
        os.chdir(path)

    def mkdir(self, path):
        os.mkdir(path)

    def external_path(self, path):
        return os.path.abspath(path)

    def setup_from_vcs(self, tree, include_controldir=None, subdir="package"):
        if self.es is None:
            raise RuntimeError("not in context manager")
        from ..vcs import dupe_vcs_tree, export_vcs_tree

        if include_controldir is False or (
            not hasattr(tree, "base") and include_controldir is None
        ):
            td = self.es.enter_context(tempfile.TemporaryDirectory())
            export_vcs_tree(tree, td)
            return td, td
        elif not hasattr(tree, "base"):
            td = self.es.enter_context(tempfile.TemporaryDirectory())
            dupe_vcs_tree(tree, td)
            return td, td
        else:
            return tree.base, tree.base

    def setup_from_directory(self, path):
        return path, path

    is_temporary = False

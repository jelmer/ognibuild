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

from . import Session

import os
import subprocess


class PlainSession(Session):
    """Session ignoring user."""

    location = "/"

    def __repr__(self):
        return "%s()" % (type(self).__name__, )

    def create_home(self):
        pass

    def check_call(self, args):
        return subprocess.check_call(args)

    def check_output(self, args):
        return subprocess.check_output(args)

    def Popen(self, args, stdout=None, stderr=None, user=None, cwd=None):
        return subprocess.Popen(args, stdout=stdout, stderr=stderr, cwd=cwd)

    def exists(self, path):
        return os.path.exists(path)

    def scandir(self, path):
        return os.scandir(path)

    def chdir(self, path):
        os.chdir(path)

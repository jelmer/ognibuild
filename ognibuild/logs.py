#!/usr/bin/python
# Copyright (C) 2018 Jelmer Vernooij <jelmer@jelmer.uk>
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

from contextlib import contextmanager
import subprocess
import logging
import os
import sys


@contextmanager
def copy_output(output_log: str, tee: bool = False):
    old_stdout = os.dup(sys.stdout.fileno())
    old_stderr = os.dup(sys.stderr.fileno())
    if tee:
        p = subprocess.Popen(["tee", output_log], stdin=subprocess.PIPE)
        newfd = p.stdin
    else:
        newfd = open(output_log, 'wb')  # noqa: SIM115
    os.dup2(newfd.fileno(), sys.stdout.fileno())  # type: ignore
    os.dup2(newfd.fileno(), sys.stderr.fileno())  # type: ignore
    try:
        yield
    finally:
        sys.stdout.flush()
        sys.stderr.flush()
        os.dup2(old_stdout, sys.stdout.fileno())
        os.dup2(old_stderr, sys.stderr.fileno())
        if newfd is not None:
            newfd.close()


@contextmanager
def redirect_output(to_file):
    sys.stdout.flush()
    sys.stderr.flush()
    old_stdout = os.dup(sys.stdout.fileno())
    old_stderr = os.dup(sys.stderr.fileno())
    os.dup2(to_file.fileno(), sys.stdout.fileno())  # type: ignore
    os.dup2(to_file.fileno(), sys.stderr.fileno())  # type: ignore
    try:
        yield
    finally:
        sys.stdout.flush()
        sys.stderr.flush()
        os.dup2(old_stdout, sys.stdout.fileno())
        os.dup2(old_stderr, sys.stderr.fileno())


def rotate_logfile(source_path: str) -> None:
    if os.path.exists(source_path):
        (directory_path, name) = os.path.split(source_path)
        i = 1
        while os.path.exists(
                os.path.join(directory_path, "%s.%d" % (name, i))):
            i += 1
        target_path = os.path.join(directory_path, "%s.%d" % (name, i))
        os.rename(source_path, target_path)
        logging.debug("Storing previous build log at %s", target_path)


class LogManager:

    def wrap(self, fn):
        raise NotImplementedError(self.wrap)


class DirectoryLogManager(LogManager):

    def __init__(self, path, mode):
        self.path = path
        self.mode = mode

    def wrap(self, fn):
        def _run(*args, **kwargs):
            rotate_logfile(self.path)
            if self.mode == 'copy':
                with copy_output(self.path, tee=True):
                    return fn(*args, **kwargs)
            elif self.mode == 'redirect':
                with copy_output(self.path, tee=False):
                    return fn(*args, **kwargs)
            else:
                raise NotImplementedError(self.mode)
        return _run


class NoLogManager(LogManager):

    def wrap(self, fn):
        return fn

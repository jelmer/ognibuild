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

import os
import logging
import shutil
import time


class DistNoTarball(Exception):
    """Dist operation did not create a tarball."""


SUPPORTED_DIST_EXTENSIONS = [
    ".tar.gz",
    ".tgz",
    ".tar.bz2",
    ".tar.xz",
    ".tar.lzma",
    ".tbz2",
    ".tar",
    ".zip",
]


def is_dist_file(fn):
    for ext in SUPPORTED_DIST_EXTENSIONS:
        if fn.endswith(ext):
            return True
    return False


class DistCatcher(object):
    def __init__(self, directories):
        self.directories = [os.path.abspath(d) for d in directories]
        self.files = []
        self.existing_files = None
        self.start_time = time.time()

    @classmethod
    def default(cls, directory):
        return cls(
            [os.path.join(directory, "dist"), directory,
             os.path.join(directory, "..")]
        )

    def __enter__(self):
        self.existing_files = {}
        for directory in self.directories:
            try:
                self.existing_files[directory] = {
                    entry.name: entry for entry in os.scandir(directory)
                }
            except FileNotFoundError:
                self.existing_files[directory] = {}
        return self

    def find_files(self):
        for directory in self.directories:
            old_files = self.existing_files[directory]
            possible_new = []
            possible_updated = []
            if not os.path.isdir(directory):
                continue
            for entry in os.scandir(directory):
                if not entry.is_file() or not is_dist_file(entry.name):
                    continue
                old_entry = old_files.get(entry.name)
                if not old_entry:
                    possible_new.append(entry)
                    continue
                if entry.stat().st_mtime > self.start_time:
                    possible_updated.append(entry)
                    continue
            if len(possible_new) == 1:
                entry = possible_new[0]
                logging.info(
                    "Found new tarball %s in %s.", entry.name, directory)
                self.files.append(entry.path)
                return entry.name
            elif len(possible_new) > 1:
                logging.warning(
                    "Found multiple tarballs %r in %s.", possible_new,
                    directory
                )
                self.files.extend([entry.path for entry in possible_new])
                return possible_new[0].name

            if len(possible_updated) == 1:
                entry = possible_updated[0]
                logging.info(
                    "Found updated tarball %s in %s.", entry.name,
                    directory)
                self.files.append(entry.path)
                return entry.name

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.find_files()
        return False

    def copy_single(self, target_dir):
        for path in self.files:
            try:
                shutil.copy(path, target_dir)
            except shutil.SameFileError:
                pass
            return os.path.basename(path)
        logging.info("No tarball created :(")
        raise DistNoTarball()

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

import errno

from breezy.errors import NotBranchError
from breezy.export import export
from breezy.workingtree import WorkingTree

from buildlog_consultant.sbuild import (
    NoSpaceOnDevice,
)

from . import DetailedFailure


def export_vcs_tree(tree, directory, subpath=""):
    try:
        export(tree, directory, "dir", None, subdir=(subpath or None))
    except OSError as e:
        if e.errno == errno.ENOSPC:
            raise DetailedFailure(1, ["export"], NoSpaceOnDevice()) from e
        raise


def dupe_vcs_tree(tree, directory):
    with tree.lock_read():
        if isinstance(tree, WorkingTree):
            tree = tree.basis_tree()
    try:
        result = tree._repository.controldir.sprout(
            directory, create_tree_if_local=True,
            revision_id=tree.get_revision_id()
        )
    except OSError as e:
        if e.errno == errno.ENOSPC:
            raise DetailedFailure(1, ["sprout"], NoSpaceOnDevice()) from e
        raise
    if not result.has_workingtree():
        raise AssertionError
    # Copy parent location - some scripts need this
    if isinstance(tree, WorkingTree):
        parent = tree.branch.get_parent()
    else:
        try:
            parent = tree._repository.controldir.open_branch().get_parent()
        except NotBranchError:
            parent = None
    if parent:
        result.open_branch().set_parent(parent)

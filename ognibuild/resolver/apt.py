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

import posixpath

from ..apt import AptManager

from . import Resolver
from ..requirements import (
    BinaryRequirement,
    PythonPackageRequirement,
    )


class NoAptPackage(Exception):
    """No apt package."""


def resolve_binary_req(apt_mgr, req):
    if posixpath.isabs(req.binary_name):
        paths = [req.binary_name]
    else:
        paths = [
            posixpath.join(dirname, req.binary_name)
            for dirname in ["/usr/bin", "/bin"]
        ]
    return apt_mgr.get_package_for_paths(paths)


APT_REQUIREMENT_RESOLVERS = [
    (BinaryRequirement, resolve_binary_req),
]


class AptResolver(Resolver):

    def __init__(self, apt):
        self.apt = apt

    @classmethod
    def from_session(cls, session):
        return cls(AptManager(session))

    def install(self, requirements):
        missing = []
        for req in requirements:
            try:
                pps = list(req.possible_paths())
            except NotImplementedError:
                missing.append(req)
            else:
                if not pps or not any(self.apt.session.exists(p) for p in pps):
                    missing.append(req)
        if missing:
            self.apt.install(list(self.resolve(missing)))

    def explain(self, requirements):
        raise NotImplementedError(self.explain)

    def resolve(self, requirements):
        for req in requirements:
            for rr_class, rr_fn in APT_REQUIREMENT_RESOLVERS:
                if isinstance(req, rr_class):
                    package_name = rr_fn(self.apt, req)
                    if package_name is None:
                        raise NoAptPackage()
                    yield package_name
                    break
            else:
                raise NotImplementedError

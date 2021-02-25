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


class MissingDependencies(Exception):

    def __init__(self, reqs):
        self.requirements = reqs


class Resolver(object):

    def install(self, requirements):
        raise NotImplementedError(self.install)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)

    def met(self, requirement):
        raise NotImplementedError(self.met)


class NativeResolver(Resolver):
    def __init__(self, session):
        self.session = session

    @classmethod
    def from_session(cls, session):
        return cls(session)

    def install(self, requirements):
        raise NotImplementedError(self.install)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)


class ExplainResolver(Resolver):
    def __init__(self, session):
        self.session = session

    @classmethod
    def from_session(cls, session):
        return cls(session)

    def install(self, requirements):
        raise MissingDependencies(requirements)


class AutoResolver(Resolver):
    """Automatically find out the most appropriate way to install dependencies.
    """

    def __init__(self, session):
        self.session = session

    @classmethod
    def from_session(cls, session):
        return cls(session)

    def install(self, requirements):
        raise NotImplementedError(self.install)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)

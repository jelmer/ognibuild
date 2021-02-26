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


class CPANResolver(object):

    def __init__(self, session):
        self.session = session

    def install(self, requirements):
        from ..requirements import PerlModuleRequirement
        missing = []
        for requirement in requirements:
            if not isinstance(requirement, PerlModuleRequirement):
                missing.append(requirement)
                continue
            # TODO(jelmer): Specify -T to skip tests?
            self.session.check_call(
                ["cpan", "-i", requirement.module],
                user="root", env={"PERL_MM_USE_DEFAULT": "1"}
            )
        if missing:
            raise MissingDependencies(missing)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)

    def met(self, requirement):
        raise NotImplementedError(self.met)


class PypiResolver(object):

    def __init__(self, session):
        self.session = session

    def install(self, requirements):
        from ..requirements import PythonPackageRequirement
        missing = []
        for requirement in requirements:
            if not isinstance(requirement, PythonPackageRequirement):
                missing.append(requirement)
                continue
            self.session.check_call(["pip", "install", requirement.package])
        if missing:
            raise MissingDependencies(missing)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)

    def met(self, requirement):
        raise NotImplementedError(self.met)


NPM_COMMAND_PACKAGES = {
    "del-cli": "del-cli",
}


class NpmResolver(object):

    def __init__(self, session):
        self.session = session

    def install(self, requirements):
        from ..requirements import NodePackageRequirement
        missing = []
        for requirement in requirements:
            if not isinstance(requirement, NodePackageRequirement):
                missing.append(requirement)
                continue
            try:
                package = NPM_COMMAND_PACKAGES[requirement.command]
            except KeyError:
                missing.append(requirement)
                continue
            self.session.check_call(["npm", "-g", "install", package])
        if missing:
            raise MissingDependencies(missing)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)

    def met(self, requirement):
        raise NotImplementedError(self.met)


class StackedResolver(Resolver):
    def __init__(self, subs):
        self.subs = subs

    def install(self, requirements):
        for sub in self.subs:
            try:
                sub.install(requirements)
            except MissingDependencies as e:
                requirements = e.requirements
            else:
                return


def native_resolvers(session):
    return StackedResolver([
        CPANResolver(session),
        PypiResolver(session),
        NpmResolver(session)])


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

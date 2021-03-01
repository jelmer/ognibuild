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


class UnsatisfiedRequirements(Exception):
    def __init__(self, reqs):
        self.requirements = reqs


class Resolver(object):
    def install(self, requirements):
        raise NotImplementedError(self.install)

    def resolve(self, requirement):
        raise NotImplementedError(self.resolve)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)

    def met(self, requirement):
        raise NotImplementedError(self.met)


class CPANResolver(Resolver):
    def __init__(self, session):
        self.session = session

    def __str__(self):
        return "cpan"

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
                user="root",
                env={"PERL_MM_USE_DEFAULT": "1"},
            )
        if missing:
            raise UnsatisfiedRequirements(missing)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)


class HackageResolver(Resolver):
    def __init__(self, session):
        self.session = session

    def __str__(self):
        return "hackage"

    def install(self, requirements):
        from ..requirements import HaskellPackageRequirement

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, HaskellPackageRequirement):
                missing.append(requirement)
                continue
            self.session.check_call(
                ["cabal", "install", requirement.package], user="root"
            )
        if missing:
            raise UnsatisfiedRequirements(missing)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)


class CargoResolver(Resolver):
    def __init__(self, session):
        self.session = session

    def __str__(self):
        return "cargo"

    def install(self, requirements):
        from ..requirements import CargoCrateRequirement

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, CargoCrateRequirement):
                missing.append(requirement)
                continue
            self.session.check_call(
                ["cargo", "install", requirement.crate], user="root"
            )
        if missing:
            raise UnsatisfiedRequirements(missing)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)


class PypiResolver(Resolver):
    def __init__(self, session):
        self.session = session

    def __str__(self):
        return "pypi"

    def install(self, requirements):
        from ..requirements import PythonPackageRequirement

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, PythonPackageRequirement):
                missing.append(requirement)
                continue
            self.session.check_call(["pip", "install", requirement.package])
        if missing:
            raise UnsatisfiedRequirements(missing)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)


NPM_COMMAND_PACKAGES = {
    "del-cli": "del-cli",
}


class NpmResolver(Resolver):
    def __init__(self, session):
        self.session = session

    def __str__(self):
        return "npm"

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
            raise UnsatisfiedRequirements(missing)

    def explain(self, requirements):
        raise NotImplementedError(self.explain)


class StackedResolver(Resolver):
    def __init__(self, subs):
        self.subs = subs

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.subs)

    def __str__(self):
        return "[" + ", ".join(map(str, self.subs)) + "]"

    def install(self, requirements):
        for sub in self.subs:
            try:
                sub.install(requirements)
            except UnsatisfiedRequirements as e:
                requirements = e.requirements
            else:
                return


def native_resolvers(session):
    return StackedResolver(
        [
            CPANResolver(session),
            PypiResolver(session),
            NpmResolver(session),
            CargoResolver(session),
            HackageResolver(session),
        ]
    )


class ExplainResolver(Resolver):
    def __init__(self, session):
        self.session = session

    @classmethod
    def from_session(cls, session):
        return cls(session)

    def install(self, requirements):
        raise UnsatisfiedRequirements(requirements)


def auto_resolver(session):
    # TODO(jelmer): if session is SchrootSession or if we're root, use apt
    from .apt import AptResolver
    from ..session.schroot import SchrootSession

    user = session.check_output(["echo", "$USER"]).decode().strip()
    resolvers = []
    if isinstance(session, SchrootSession) or user == "root":
        resolvers.append(AptResolver.from_session(session))
    resolvers.extend(
        [
            CPANResolver(session),
            PypiResolver(session),
            NpmResolver(session),
            CargoResolver(session),
            HackageResolver(session),
        ]
    )
    return StackedResolver(resolvers)

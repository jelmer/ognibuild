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


import subprocess


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

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.session)

    def explain(self, requirements):
        from ..requirements import PerlModuleRequirement

        perlreqs = []
        for requirement in requirements:
            if not isinstance(requirement, PerlModuleRequirement):
                continue
            perlreqs.append(requirement)
        if perlreqs:
            yield (["cpan", "-i"] + [req.module for req in perlreqs], [perlreqs])

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
                env={"PERL_MM_USE_DEFAULT": "1"},
            )
        if missing:
            raise UnsatisfiedRequirements(missing)


class HackageResolver(Resolver):
    def __init__(self, session):
        self.session = session

    def __str__(self):
        return "hackage"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.session)

    def install(self, requirements):
        from ..requirements import HaskellPackageRequirement

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, HaskellPackageRequirement):
                missing.append(requirement)
                continue
            self.session.check_call(
                ["cabal", "install", requirement.package]
            )
        if missing:
            raise UnsatisfiedRequirements(missing)

    def explain(self, requirements):
        from ..requirements import HaskellPackageRequirement

        haskellreqs = []
        for requirement in requirements:
            if not isinstance(requirement, HaskellPackageRequirement):
                continue
            haskellreqs.append(requirement)
        if haskellreqs:
            yield (["cabal", "install"] + [req.package for req in haskellreqs],
                   haskellreqs)


class PypiResolver(Resolver):
    def __init__(self, session):
        self.session = session

    def __str__(self):
        return "pypi"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.session)

    def install(self, requirements):
        from ..requirements import PythonPackageRequirement

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, PythonPackageRequirement):
                missing.append(requirement)
                continue
            try:
                self.session.check_call(
                    ["pip", "install", requirement.package])
            except subprocess.CalledProcessError:
                missing.append(requirement)
        if missing:
            raise UnsatisfiedRequirements(missing)

    def explain(self, requirements):
        from ..requirements import PythonPackageRequirement

        pyreqs = []
        for requirement in requirements:
            if not isinstance(requirement, PythonPackageRequirement):
                continue
            pyreqs.append(requirement)
        if pyreqs:
            yield (["pip", "install"] + [req.package for req in pyreqs],
                   pyreqs)


class GoResolver(Resolver):

    def __init__(self, session):
        self.session = session

    def __str__(self):
        return "go"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.session)

    def install(self, requirements):
        from ..requirements import GoPackageRequirement

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, GoPackageRequirement):
                missing.append(requirement)
                continue
            self.session.check_call(["go", "get", requirement.package])
        if missing:
            raise UnsatisfiedRequirements(missing)

    def explain(self, requirements):
        from ..requirements import GoPackageRequirement

        goreqs = []
        for requirement in requirements:
            if not isinstance(requirement, GoPackageRequirement):
                continue
            goreqs.append(requirement)
        if goreqs:
            yield (["go", "get"] + [req.package for req in goreqs],
                   goreqs)


NPM_COMMAND_PACKAGES = {
    "del-cli": "del-cli",
}


class NpmResolver(Resolver):
    def __init__(self, session):
        self.session = session

    def __str__(self):
        return "npm"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.session)

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
        from ..requirements import NodePackageRequirement

        nodereqs = []
        packages = []
        for requirement in requirements:
            if not isinstance(requirement, NodePackageRequirement):
                continue
            try:
                package = NPM_COMMAND_PACKAGES[requirement.command]
            except KeyError:
                continue
            nodereqs.append(requirement)
            packages.append(package)
        if nodereqs:
            yield (["npm", "-g", "install"] + packages, nodereqs)


class StackedResolver(Resolver):
    def __init__(self, subs):
        self.subs = subs

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.subs)

    def __str__(self):
        return "[" + ", ".join(map(str, self.subs)) + "]"

    def explain(self, requirements):
        for sub in self.subs:
            yield from sub.explain(requirements)

    def install(self, requirements):
        for sub in self.subs:
            try:
                sub.install(requirements)
            except UnsatisfiedRequirements as e:
                requirements = e.requirements
            else:
                return


NATIVE_RESOLVER_CLS = [
    CPANResolver,
    PypiResolver,
    NpmResolver,
    GoResolver,
    HackageResolver,
    ]


def native_resolvers(session):
    return StackedResolver([kls(session) for kls in NATIVE_RESOLVER_CLS])


class ExplainResolver(Resolver):
    def __init__(self, session):
        self.session = session

    @classmethod
    def from_session(cls, session):
        return cls(session)

    def install(self, requirements):
        raise UnsatisfiedRequirements(requirements)


def auto_resolver(session):
    # if session is SchrootSession or if we're root, use apt
    from .apt import AptResolver
    from ..session.schroot import SchrootSession

    user = session.check_output(["echo", "$USER"]).decode().strip()
    resolvers = []
    # TODO(jelmer): Check VIRTUAL_ENV, and prioritize PypiResolver if
    # present?
    if isinstance(session, SchrootSession) or user == "root":
        resolvers.append(AptResolver.from_session(session))
    resolvers.extend([kls(session) for kls in NATIVE_RESOLVER_CLS])
    return StackedResolver(resolvers)

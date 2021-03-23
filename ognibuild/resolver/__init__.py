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
from ..fix_build import run_detecting_problems

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

    def env(self):
        return {}


class CPANResolver(Resolver):
    def __init__(self, session, user_local=False):
        self.session = session
        self.user_local = user_local

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

        env = {
            "PERL_MM_USE_DEFAULT": "1",
            "PERL_MM_OPT": "",
            "PERL_MB_OPT": "",
            }

        if not self.user_local:
            user = "root"
        else:
            user = None

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, PerlModuleRequirement):
                missing.append(requirement)
                continue
            # TODO(jelmer): Specify -T to skip tests?
            run_detecting_problems(self.session,
                ["cpan", "-i", requirement.module],
                env=env,
                user=user,
            )
        if missing:
            raise UnsatisfiedRequirements(missing)


class RResolver(Resolver):
    def __init__(self, session, repos, user_local=False):
        self.session = session
        self.repos = repos
        self.user_local = user_local

    def __str__(self):
        return "cran"

    def __repr__(self):
        return "%s(%r, %r)" % (type(self).__name__, self.session, self.repos)

    def _cmd(self, req):
        # TODO(jelmer: Handle self.user_local
        return ["R", "-e", "install.packages('%s', repos=%r)" % (req.package, self.repos)]

    def explain(self, requirements):
        from ..requirements import RPackageRequirement

        rreqs = []
        for requirement in requirements:
            if not isinstance(requirement, RPackageRequirement):
                continue
            rreqs.append(requirement)
        if rreqs:
            yield ([self._cmd(req) for req in rreqs])

    def install(self, requirements):
        from ..requirements import RPackageRequirement

        if self.user_local:
            user = None
        else:
            user = "root"

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, RPackageRequirement):
                missing.append(requirement)
                continue
            self.session.check_call(self._cmd(requirement), user=user)
        if missing:
            raise UnsatisfiedRequirements(missing)


class OctaveForgeResolver(Resolver):
    def __init__(self, session, user_local=False):
        self.session = session
        self.user_local = user_local

    def __str__(self):
        return "octave-forge"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.session)

    def _cmd(self, req):
        # TODO(jelmer: Handle self.user_local
        return ["octave-cli", "--eval", "pkg install -forge %s" % req.package]

    def explain(self, requirements):
        from ..requirements import OctavePackageRequirement

        rreqs = []
        for requirement in requirements:
            if not isinstance(requirement, OctavePackageRequirement):
                continue
            rreqs.append(requirement)
        if rreqs:
            yield ([self._cmd(req) for req in rreqs])

    def install(self, requirements):
        from ..requirements import OctavePackageRequirement

        if self.user_local:
            user = None
        else:
            user = "root"

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, OctavePackageRequirement):
                missing.append(requirement)
                continue
            self.session.check_call(self._cmd(requirement), user=user)
        if missing:
            raise UnsatisfiedRequirements(missing)


class CRANResolver(RResolver):

    def __init__(self, session, user_local=False):
        super(CRANResolver, self).__init__(session, 'http://cran.r-project.org', user_local=user_local)


class BioconductorResolver(RResolver):

    def __init__(self, session, user_local=False):
        super(BioconductorResolver, self).__init__(
            session, 'https://hedgehog.fhcrc.org/bioconductor', user_local=user_local)


class HackageResolver(Resolver):
    def __init__(self, session, user_local=False):
        self.session = session
        self.user_local = user_local

    def __str__(self):
        return "hackage"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.session)

    def _cmd(self, reqs):
        extra_args = []
        if self.user_local:
            extra_args.append('--user')
        return ["cabal", "install"] + extra_args + [req.package for req in reqs]

    def install(self, requirements):
        from ..requirements import HaskellPackageRequirement

        if self.user_local:
            user = None
        else:
            user = "root"

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, HaskellPackageRequirement):
                missing.append(requirement)
                continue
            self.session.check_call(self._cmd([requirement]), user=user)
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
            yield (self._cmd(haskellreqs), haskellreqs)


class PypiResolver(Resolver):
    def __init__(self, session, user_local=False):
        self.session = session
        self.user_local = user_local

    def __str__(self):
        return "pypi"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.session)

    def _cmd(self, reqs):
        extra_args = []
        if self.user_local:
            extra_args.append('--user')
        return ["pip", "install"] + extra_args + [req.package for req in reqs]

    def install(self, requirements):
        from ..requirements import PythonPackageRequirement

        if self.user_local:
            user = None
        else:
            user = "root"

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, PythonPackageRequirement):
                missing.append(requirement)
                continue
            try:
                self.session.check_call(
                    self._cmd([requirement]), user=user)
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
            yield (self._cmd(pyreqs), pyreqs)


class GoResolver(Resolver):

    def __init__(self, session, user_local):
        self.session = session
        self.user_local = user_local

    def __str__(self):
        return "go"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.session)

    def install(self, requirements):
        from ..requirements import GoPackageRequirement

        if self.user_local:
            env = {}
        else:
            # TODO(jelmer): Isn't this Debian-specific?
            env = {'GOPATH': '/usr/share/gocode'}

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, GoPackageRequirement):
                missing.append(requirement)
                continue
            self.session.check_call(["go", "get", requirement.package], env=env)
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
    "husky": "husky",
}


class NpmResolver(Resolver):
    def __init__(self, session, user_local=False):
        self.session = session
        self.user_local = user_local
        # TODO(jelmer): Handle user_local

    def __str__(self):
        return "npm"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.session)

    def install(self, requirements):
        from ..requirements import (
            NodePackageRequirement,
            NodeModuleRequirement,
            BinaryRequirement,
            )

        missing = []
        for requirement in requirements:
            if isinstance(requirement, BinaryRequirement):
                try:
                    package = NPM_COMMAND_PACKAGES[requirement.binary_name]
                except KeyError:
                    pass
                else:
                    requirement = NodePackageRequirement(package)
            if isinstance(requirement, NodeModuleRequirement):
                # TODO: Is this legit?
                requirement = NodePackageRequirement(requirement.module.split('/')[0])
            if not isinstance(requirement, NodePackageRequirement):
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

    def env(self):
        ret = {}
        # Reversed so earlier resolvers override later ones
        for sub in reversed(self.subs):
            ret.update(sub.env())
        return ret

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
    CRANResolver,
    BioconductorResolver,
    OctaveForgeResolver,
    ]


def native_resolvers(session, user_local):
    return StackedResolver([kls(session, user_local) for kls in NATIVE_RESOLVER_CLS])


def auto_resolver(session, explain=False):
    # if session is SchrootSession or if we're root, use apt
    from .apt import AptResolver
    from ..session.schroot import SchrootSession
    from ..session import get_user

    user = get_user(session)
    resolvers = []
    # TODO(jelmer): Check VIRTUAL_ENV, and prioritize PypiResolver if
    # present?
    if isinstance(session, SchrootSession) or user == "root" or explain:
        user_local = False
    else:
        user_local = True
    if not user_local:
        resolvers.append(AptResolver.from_session(session))
    resolvers.extend([kls(session, user_local) for kls in NATIVE_RESOLVER_CLS])
    return StackedResolver(resolvers)

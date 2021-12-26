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


import logging
import subprocess
from typing import Optional

from .. import UnidentifiedError
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
    def __init__(self, session, user_local=False, skip_tests=True):
        self.session = session
        self.user_local = user_local
        self.skip_tests = skip_tests

    def __str__(self):
        return "cpan"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.session)

    def _cmd(self, reqs):
        ret = ["cpan", "-i"]
        if self.skip_tests:
            ret.append("-T")
        ret.extend([req.module for req in reqs])
        return ret

    def explain(self, requirements):
        from ..requirements import PerlModuleRequirement

        perlreqs = []
        for requirement in requirements:
            if not isinstance(requirement, PerlModuleRequirement):
                continue
            perlreqs.append(requirement)
        if perlreqs:
            yield (self._cmd(perlreqs), perlreqs)

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
            cmd = self._cmd([requirement])
            logging.info("CPAN: running %r", cmd)
            run_detecting_problems(
                self.session,
                cmd,
                env=env,
                user=user,
            )
        if missing:
            raise UnsatisfiedRequirements(missing)


class TlmgrResolver(Resolver):
    def __init__(self, session, repository: str, user_local=False):
        self.session = session
        self.user_local = user_local
        self.repository = repository

    def __str__(self):
        if self.repository.startswith('http://') or self.repository.startswith('https://'):
            return 'tlmgr(%r)' % self.repository
        else:
            return self.repository

    def __repr__(self):
        return "%s(%r, %r)" % (
            type(self).__name__, self.session, self.repository)

    def _cmd(self, reqs):
        ret = ["tlmgr", "--repository=%s" % self.repository, "install"]
        if self.user_local:
            ret.append("--usermode")
        ret.extend([req.package for req in reqs])
        return ret

    def explain(self, requirements):
        from ..requirements import LatexPackageRequirement

        latexreqs = []
        for requirement in requirements:
            if not isinstance(requirement, LatexPackageRequirement):
                continue
            latexreqs.append(requirement)
        if latexreqs:
            yield (self._cmd(latexreqs), latexreqs)

    def install(self, requirements):
        from ..requirements import LatexPackageRequirement

        if not self.user_local:
            user = "root"
        else:
            user = None

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, LatexPackageRequirement):
                missing.append(requirement)
                continue
            cmd = self._cmd([requirement])
            logging.info("tlmgr: running %r", cmd)
            try:
                run_detecting_problems(self.session, cmd, user=user)
            except UnidentifiedError as e:
                if "tlmgr: user mode not initialized, please read the documentation!" in e.lines:
                    self.session.check_call(['tlmgr', 'init-usertree'])
                else:
                    raise
        if missing:
            raise UnsatisfiedRequirements(missing)


class CTANResolver(TlmgrResolver):

    def __init__(self, session, user_local=False):
        super(CTANResolver, self).__init__(
            session, "ctan", user_local=user_local)


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
        return [
            "R",
            "-e",
            "install.packages('%s', repos=%r)" % (req.package, self.repos),
        ]

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
            cmd = self._cmd(requirement)
            logging.info("RResolver(%r): running %r", self.repos, cmd)
            run_detecting_problems(self.session, cmd, user=user)
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
            cmd = self._cmd(requirement)
            logging.info("Octave: running %r", cmd)
            run_detecting_problems(self.session, cmd, user=user)
        if missing:
            raise UnsatisfiedRequirements(missing)


class CRANResolver(RResolver):
    def __init__(self, session, user_local=False):
        super(CRANResolver, self).__init__(
            session, "http://cran.r-project.org", user_local=user_local
        )


class BioconductorResolver(RResolver):
    def __init__(self, session, user_local=False):
        super(BioconductorResolver, self).__init__(
            session, "https://hedgehog.fhcrc.org/bioconductor", user_local=user_local
        )


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
            extra_args.append("--user")
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
            cmd = self._cmd([requirement])
            logging.info("Hackage: running %r", cmd)
            run_detecting_problems(self.session, cmd, user=user)
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
            extra_args.append("--user")
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
            cmd = self._cmd([requirement])
            logging.info("pip: running %r", cmd)
            try:
                run_detecting_problems(self.session, cmd, user=user)
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
            env = {"GOPATH": "/usr/share/gocode"}

        missing = []
        for requirement in requirements:
            if not isinstance(requirement, GoPackageRequirement):
                missing.append(requirement)
                continue
            cmd = ["go", "get", requirement.package]
            logging.info("go: running %r", cmd)
            run_detecting_problems(self.session, cmd, env=env)
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
            yield (["go", "get"] + [req.package for req in goreqs], goreqs)


NPM_COMMAND_PACKAGES = {
    "del-cli": "del-cli",
    "husky": "husky",
    "cross-env": "cross-env",
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

        if self.user_local:
            user = None
        else:
            user = "root"

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
                parts = requirement.module.split("/")
                if parts[0].startswith('@'):
                    requirement = NodePackageRequirement('/'.join(parts[:2]))
                else:
                    requirement = NodePackageRequirement(parts[0])
            if not isinstance(requirement, NodePackageRequirement):
                missing.append(requirement)
                continue
            cmd = ["npm", "-g", "install", requirement.package]
            logging.info("npm: running %r", cmd)
            run_detecting_problems(self.session, cmd, user=user)
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
        if requirements:
            raise UnsatisfiedRequirements(requirements)


NATIVE_RESOLVER_CLS = [
    CPANResolver,
    CTANResolver,
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


def auto_resolver(session, explain=False, system_wide: Optional[bool] = None):
    # if session is SchrootSession or if we're root, use apt
    from ..session.schroot import SchrootSession
    from ..session import get_user

    user = get_user(session)
    resolvers = []
    if system_wide is None:
        # TODO(jelmer): Check VIRTUAL_ENV, and prioritize PypiResolver if
        # present?
        if isinstance(session, SchrootSession) or user == "root" or explain:
            system_wide = True
        else:
            system_wide = False
    if system_wide:
        try:
            from .apt import AptResolver
        except ModuleNotFoundError:
            pass
        else:
            resolvers.append(AptResolver.from_session(session))
    resolvers.extend([kls(session, not system_wide) for kls in NATIVE_RESOLVER_CLS])
    return StackedResolver(resolvers)

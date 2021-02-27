#!/usr/bin/python
# Copyright (C) 2019-2020 Jelmer Vernooij <jelmer@jelmer.uk>
# encoding: utf-8
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
import os
import re
from typing import Optional
import warnings

from . import shebang_binary, UpstreamOutput, UnidentifiedError
from .requirements import (
    BinaryRequirement,
    PythonPackageRequirement,
    PerlModuleRequirement,
    NodePackageRequirement,
    CargoCrateRequirement,
    )
from .fix_build import run_with_build_fixers


class NoBuildToolsFound(Exception):
    """No supported build tools were found."""


class InstallTarget(object):

    # Whether to prefer user-specific installation
    user: Optional[bool]

    # TODO(jelmer): Add information about target directory, layout, etc.


class BuildSystem(object):
    """A particular buildsystem."""

    name: str

    def dist(self, session, resolver, fixers):
        raise NotImplementedError(self.dist)

    def test(self, session, resolver, fixers):
        raise NotImplementedError(self.test)

    def build(self, session, resolver, fixers):
        raise NotImplementedError(self.build)

    def clean(self, session, resolver, fixers):
        raise NotImplementedError(self.clean)

    def install(self, session, resolver, fixers, install_target):
        raise NotImplementedError(self.install)

    def get_declared_dependencies(self):
        raise NotImplementedError(self.get_declared_dependencies)

    def get_declared_outputs(self):
        raise NotImplementedError(self.get_declared_outputs)


class Pear(BuildSystem):

    name = "pear"

    def __init__(self, path):
        self.path = path

    def setup(self, resolver):
        resolver.install([BinaryRequirement("pear")])

    def dist(self, session, resolver, fixers):
        self.setup(resolver)
        run_with_build_fixers(session, ["pear", "package"], fixers)

    def test(self, session, resolver, fixers):
        self.setup(resolver)
        run_with_build_fixers(session, ["pear", "run-tests"], fixers)

    def build(self, session, resolver, fixers):
        self.setup(resolver)
        run_with_build_fixers(session, ["pear", "build", self.path], fixers)

    def clean(self, session, resolver, fixers):
        self.setup(resolver)
        # TODO

    def install(self, session, resolver, fixers, install_target):
        self.setup(resolver)
        run_with_build_fixers(session, ["pear", "install", self.path], fixers)


class SetupPy(BuildSystem):

    name = "setup.py"

    def __init__(self, path):
        self.path = path
        from distutils.core import run_setup
        self.result = run_setup(os.path.abspath(path), stop_after="init")

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def setup(self, resolver):
        resolver.install([PythonPackageRequirement('pip')])
        with open(self.path, "r") as f:
            setup_py_contents = f.read()
        try:
            with open("setup.cfg", "r") as f:
                setup_cfg_contents = f.read()
        except FileNotFoundError:
            setup_cfg_contents = ""
        if "setuptools" in setup_py_contents:
            logging.info("Reference to setuptools found, installing.")
            resolver.install([PythonPackageRequirement("setuptools")])
        if (
            "setuptools_scm" in setup_py_contents
            or "setuptools_scm" in setup_cfg_contents
        ):
            logging.info("Reference to setuptools-scm found, installing.")
            resolver.install(
                [
                    PythonPackageRequirement("setuptools-scm"),
                    BinaryRequirement("git"),
                    BinaryRequirement("mercurial"),
                ]
            )

        # TODO(jelmer): Install setup_requires

    def test(self, session, resolver, fixers):
        self.setup(resolver)
        self._run_setup(session, resolver, ["test"], fixers)

    def build(self, session, resolver, fixers):
        self.setup(resolver)
        self._run_setup(session, resolver, ["build"], fixers)

    def dist(self, session, resolver, fixers):
        self.setup(resolver)
        self._run_setup(session, resolver, ["sdist"], fixers)

    def clean(self, session, resolver, fixers):
        self.setup(resolver)
        self._run_setup(session, resolver, ["clean"], fixers)

    def install(self, session, resolver, fixers, install_target):
        self.setup(resolver)
        extra_args = []
        if install_target.user:
            extra_args.append('--user')
        self._run_setup(session, resolver, ["install"] + extra_args, fixers)

    def _run_setup(self, session, resolver, args, fixers):
        interpreter = shebang_binary("setup.py")
        if interpreter is not None:
            resolver.install([BinaryRequirement(interpreter)])
            run_with_build_fixers(session, ["./setup.py"] + args, fixers)
        else:
            # Just assume it's Python 3
            resolver.install([BinaryRequirement("python3")])
            run_with_build_fixers(
                session, ["python3", "./setup.py"] + args,
                fixers)

    def get_declared_dependencies(self):
        for require in self.result.get_requires():
            yield "build", PythonPackageRequirement(require)
        # Not present for distutils-only packages
        if getattr(self.result, 'install_requires', []):
            for require in self.result.install_requires:
                yield "install", PythonPackageRequirement(require)
        # Not present for distutils-only packages
        if getattr(self.result, 'tests_require', []):
            for require in self.result.tests_require:
                yield "test", PythonPackageRequirement(require)

    def get_declared_outputs(self):
        for script in self.result.scripts or []:
            yield UpstreamOutput("binary", os.path.basename(script))
        entry_points = getattr(self.result, 'entry_points', None) or {}
        for script in entry_points.get("console_scripts", []):
            yield UpstreamOutput("binary", script.split("=")[0])
        for package in self.result.packages or []:
            yield UpstreamOutput("python3", package)


class PyProject(BuildSystem):

    name = "pyproject"

    def __init__(self, path):
        self.path = path
        self.pyproject = self.load_toml()

    def load_toml(self):
        import toml

        with open(self.path, "r") as pf:
            return toml.load(pf)

    def dist(self, session, resolver, fixers):
        if "poetry" in self.pyproject.get("tool", []):
            logging.info(
                "Found pyproject.toml with poetry section, " "assuming poetry project."
            )
            resolver.install(
                [
                    PythonPackageRequirement("venv"),
                    PythonPackageRequirement("pip"),
                ]
            )
            session.check_call(["pip3", "install", "poetry"], user="root")
            session.check_call(["poetry", "build", "-f", "sdist"])
            return
        raise AssertionError("no supported section in pyproject.toml")


class SetupCfg(BuildSystem):

    name = "setup.cfg"

    def __init__(self, path):
        self.path = path

    def setup(self, resolver):
        resolver.install(
            [
                PythonPackageRequirement("pep517"),
                PythonPackageRequirement("pip"),
            ]
        )

    def dist(self, session, resolver, fixers):
        self.setup(resolver)
        session.check_call(["python3", "-m", "pep517.build", "-s", "."])


class Npm(BuildSystem):

    name = "npm"

    def __init__(self, path):
        import json

        with open(path, "r") as f:
            self.package = json.load(f)

    def get_declared_dependencies(self):
        if "devDependencies" in self.package:
            for name, unused_version in self.package["devDependencies"].items():
                # TODO(jelmer): Look at version
                yield "dev", NodePackageRequirement(name)

    def setup(self, resolver):
        resolver.install([BinaryRequirement("npm")])

    def dist(self, session, resolver, fixers):
        self.setup(resolver)
        run_with_build_fixers(session, ["npm", "pack"], fixers)


class Waf(BuildSystem):

    name = "waf"

    def __init__(self, path):
        self.path = path

    def setup(self, resolver):
        resolver.install([BinaryRequirement("python3")])

    def dist(self, session, resolver, fixers):
        self.setup(resolver)
        run_with_build_fixers(session, ["./waf", "dist"], fixers)


class Gem(BuildSystem):

    name = "gem"

    def __init__(self, path):
        self.path = path

    def setup(self, resolver):
        resolver.install([BinaryRequirement("gem2deb")])

    def dist(self, session, resolver, fixers):
        self.setup(resolver)
        gemfiles = [
            entry.name for entry in session.scandir(".") if entry.name.endswith(".gem")
        ]
        if len(gemfiles) > 1:
            logging.warning("More than one gemfile. Trying the first?")
        run_with_build_fixers(session, ["gem2tgz", gemfiles[0]], fixers)


class DistInkt(BuildSystem):
    def __init__(self, path):
        self.path = path
        self.name = "dist-zilla"
        self.dist_inkt_class = None
        with open("dist.ini", "rb") as f:
            for line in f:
                if not line.startswith(b";;"):
                    continue
                try:
                    (key, value) = line[2:].split(b"=", 1)
                except ValueError:
                    continue
                if key.strip() == b"class" and value.strip().startswith(b"'Dist::Inkt"):
                    logging.info(
                        "Found Dist::Inkt section in dist.ini, " "assuming distinkt."
                    )
                    self.name = "dist-inkt"
                    self.dist_inkt_class = value.decode().strip("'")
                    return
        logging.info("Found dist.ini, assuming dist-zilla.")

    def setup(self, resolver):
        resolver.install(
            [
                PerlModuleRequirement("Dist::Inkt"),
            ]
        )

    def dist(self, session, resolver, fixers):
        self.setup(resolver)
        if self.name == "dist-inkt":
            resolver.install([PerlModuleRequirement(self.dist_inkt_class)])
            run_with_build_fixers(session, ["distinkt-dist"], fixers)
        else:
            # Default to invoking Dist::Zilla
            resolver.install([PerlModuleRequirement("Dist::Zilla")])
            run_with_build_fixers(
                session, ["dzil", "build", "--in", ".."], fixers)


class Make(BuildSystem):

    name = "make"

    def __repr__(self):
        return "%s()" % type(self).__name__

    def setup(self, session, resolver, fixers):
        resolver.install([BinaryRequirement("make")])

        if session.exists("Makefile.PL") and not session.exists("Makefile"):
            resolver.install([BinaryRequirement("perl")])
            run_with_build_fixers(session, ["perl", "Makefile.PL"], fixers)

        if not session.exists("Makefile") and not session.exists("configure"):
            if session.exists("autogen.sh"):
                if shebang_binary("autogen.sh") is None:
                    run_with_build_fixers(
                        session, ["/bin/sh", "./autogen.sh"], fixers)
                try:
                    run_with_build_fixers(
                        session, ["./autogen.sh"], fixers)
                except UnidentifiedError as e:
                    if (
                        "Gnulib not yet bootstrapped; "
                        "run ./bootstrap instead.\n" in e.lines
                    ):
                        run_with_build_fixers(
                            session, ["./bootstrap"], fixers)
                        run_with_build_fixers(
                            session, ["./autogen.sh"], fixers)
                    else:
                        raise

            elif session.exists("configure.ac") or session.exists("configure.in"):
                resolver.install(
                    [
                        BinaryRequirement("autoconf"),
                        BinaryRequirement("automake"),
                        BinaryRequirement("gettextize"),
                        BinaryRequirement("libtoolize"),
                    ]
                )
                run_with_build_fixers(session, ["autoreconf", "-i"], fixers)

        if not session.exists("Makefile") and session.exists("configure"):
            session.check_call(["./configure"])

    def build(self, session, resolver, fixers):
        self.setup(session, resolver)
        run_with_build_fixers(session, ["make", "all"], fixers)

    def install(self, session, resolver, fixers, install_target):
        self.setup(session, resolver)
        run_with_build_fixers(session, ["make", "install"], fixers)

    def dist(self, session, resolver, fixers):
        self.setup(session, resolver)
        try:
            run_with_build_fixers(session, ["make", "dist"], fixers)
        except UnidentifiedError as e:
            if "make: *** No rule to make target 'dist'.  Stop.\n" in e.lines:
                pass
            elif "make[1]: *** No rule to make target 'dist'. Stop.\n" in e.lines:
                pass
            elif (
                "Reconfigure the source tree "
                "(via './config' or 'perl Configure'), please.\n"
            ) in e.lines:
                run_with_build_fixers(session, ["./config"], fixers)
                run_with_build_fixers(session, ["make", "dist"], fixers)
            elif (
                "Please try running 'make manifest' and then run "
                "'make dist' again.\n" in e.lines
            ):
                run_with_build_fixers(session, ["make", "manifest"], fixers)
                run_with_build_fixers(session, ["make", "dist"], fixers)
            elif "Please run ./configure first\n" in e.lines:
                run_with_build_fixers(session, ["./configure"], fixers)
                run_with_build_fixers(session, ["make", "dist"], fixers)
            elif any(
                [
                    re.match(
                        r"Makefile:[0-9]+: \*\*\* Missing \'Make.inc\' "
                        r"Run \'./configure \[options\]\' and retry.  Stop.\n",
                        line,
                    )
                    for line in e.lines
                ]
            ):
                run_with_build_fixers(session, ["./configure"], fixers)
                run_with_build_fixers(session, ["make", "dist"], fixers)
            elif any(
                [
                    re.match(
                        r"Problem opening MANIFEST: No such file or directory "
                        r"at .* line [0-9]+\.",
                        line,
                    )
                    for line in e.lines
                ]
            ):
                run_with_build_fixers(session, ["make", "manifest"], fixers)
                run_with_build_fixers(session, ["make", "dist"], fixers)
            else:
                raise
        else:
            return

    def get_declared_dependencies(self):
        # TODO(jelmer): Split out the perl-specific stuff?
        if os.path.exists("META.yml"):
            # See http://module-build.sourceforge.net/META-spec-v1.4.html for
            # the specification of the format.
            import ruamel.yaml
            import ruamel.yaml.reader

            with open("META.yml", "rb") as f:
                try:
                    data = ruamel.yaml.load(f, ruamel.yaml.SafeLoader)
                except ruamel.yaml.reader.ReaderError as e:
                    warnings.warn("Unable to parse META.yml: %s" % e)
                    return
                for require in data.get("requires", []):
                    yield "build", PerlModuleRequirement(require)


class Cargo(BuildSystem):

    name = "cargo"

    def __init__(self, path):
        from toml.decoder import load

        with open(path, "r") as f:
            self.cargo = load(f)

    def get_declared_dependencies(self):
        if "dependencies" in self.cargo:
            for name, details in self.cargo["dependencies"].items():
                # TODO(jelmer): Look at details['features'], details['version']
                yield "build", CargoCrateRequirement(name)


class Golang(BuildSystem):
    """Go builds."""

    name = "golang"


class Maven(BuildSystem):

    name = "maven"

    def __init__(self, path):
        self.path = path


class Cabal(BuildSystem):

    name = "cabal"

    def __init__(self, path):
        self.path = path


def detect_buildsystems(path, trust_package=False):  # noqa: C901
    """Detect build systems."""
    if os.path.exists(os.path.join(path, "package.xml")):
        logging.info("Found package.xml, assuming pear package.")
        yield Pear("package.xml")

    if os.path.exists(os.path.join(path, "setup.py")):
        logging.info("Found setup.py, assuming python project.")
        yield SetupPy("setup.py")
    elif os.path.exists(os.path.join(path, "pyproject.toml")):
        logging.info("Found pyproject.toml, assuming python project.")
        yield PyProject("pyproject.toml")
    elif os.path.exists(os.path.join(path, "setup.cfg")):
        logging.info("Found setup.cfg, assuming python project.")
        yield SetupCfg("setup.cfg")

    if os.path.exists(os.path.join(path, "package.json")):
        logging.info("Found package.json, assuming node package.")
        yield Npm("package.json")

    if os.path.exists(os.path.join(path, "waf")):
        logging.info("Found waf, assuming waf package.")
        yield Waf("waf")

    if os.path.exists(os.path.join(path, "Cargo.toml")):
        logging.info("Found Cargo.toml, assuming rust cargo package.")
        yield Cargo("Cargo.toml")

    if os.path.exists(os.path.join(path, "pom.xml")):
        logging.info("Found pom.xml, assuming maven package.")
        yield Maven("pom.xml")

    if os.path.exists(os.path.join(path, "dist.ini")) and not os.path.exists(
        os.path.join(path, "Makefile.PL")
    ):
        yield DistInkt("dist.ini")

    gemfiles = [entry.name for entry in os.scandir(path) if entry.name.endswith(".gem")]
    if gemfiles:
        yield Gem(gemfiles[0])

    if any(
        [
            os.path.exists(os.path.join(path, p))
            for p in [
                "Makefile",
                "Makefile.PL",
                "autogen.sh",
                "configure.ac",
                "configure.in",
            ]
        ]
    ):
        yield Make()

    cabal_filenames = [
        entry.name for entry in os.scandir(path) if entry.name.endswith(".cabal")
    ]
    if cabal_filenames:
        if len(cabal_filenames) == 1:
            yield Cabal(cabal_filenames[0])
        else:
            warnings.warn(
                "More than one cabal filename, ignoring all: %r" % cabal_filenames
            )

    if os.path.exists(os.path.join(path, ".travis.yml")):
        import ruamel.yaml.reader

        with open(".travis.yml", "rb") as f:
            try:
                data = ruamel.yaml.load(f, ruamel.yaml.SafeLoader)
            except ruamel.yaml.reader.ReaderError as e:
                warnings.warn("Unable to parse .travis.yml: %s" % (e,))
            else:
                language = data.get("language")
                if language == "go":
                    yield Golang()

    for entry in os.scandir(path):
        if entry.name.endswith(".go"):
            yield Golang()
            break


def get_buildsystem(path, trust_package=False):
    for buildsystem in detect_buildsystems(path, trust_package=trust_package):
        return buildsystem

    raise NoBuildToolsFound()

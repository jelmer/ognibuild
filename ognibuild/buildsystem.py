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
from typing import Optional, Tuple
import warnings

from . import shebang_binary, UnidentifiedError
from .outputs import (
    BinaryOutput,
    PythonPackageOutput,
    RPackageOutput,
)
from .requirements import (
    BinaryRequirement,
    PythonPackageRequirement,
    PerlModuleRequirement,
    NodePackageRequirement,
    CargoCrateRequirement,
    RPackageRequirement,
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

    def __str__(self):
        return self.name

    def dist(self, session, resolver, fixers, quiet=False):
        raise NotImplementedError(self.dist)

    def test(self, session, resolver, fixers):
        raise NotImplementedError(self.test)

    def build(self, session, resolver, fixers):
        raise NotImplementedError(self.build)

    def clean(self, session, resolver, fixers):
        raise NotImplementedError(self.clean)

    def install(self, session, resolver, fixers, install_target):
        raise NotImplementedError(self.install)

    def get_declared_dependencies(self, session, fixers=None):
        raise NotImplementedError(self.get_declared_dependencies)

    def get_declared_outputs(self, session, fixers=None):
        raise NotImplementedError(self.get_declared_outputs)

    @classmethod
    def probe(cls, path):
        return None


class Pear(BuildSystem):

    name = "pear"

    def __init__(self, path):
        self.path = path

    def setup(self, resolver):
        resolver.install([BinaryRequirement("pear")])

    def dist(self, session, resolver, fixers, quiet=False):
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

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "package.xml")):
            logging.debug("Found package.xml, assuming pear package.")
            return cls(os.path.join(path, "package.xml"))


# run_setup, but setting __name__
# Imported from Python's distutils.core, Copyright (C) PSF

def run_setup(script_name, script_args=None, stop_after="run"):
    from distutils import core
    import sys

    if stop_after not in ("init", "config", "commandline", "run"):
        raise ValueError("invalid value for 'stop_after': %r" % (stop_after,))

    core._setup_stop_after = stop_after

    save_argv = sys.argv.copy()
    g = {"__file__": script_name, "__name__": "__main__"}
    try:
        old_cwd = os.getcwd()
        os.chdir(os.path.dirname(script_name))
        try:
            sys.argv[0] = script_name
            if script_args is not None:
                sys.argv[1:] = script_args
            with open(script_name, "rb") as f:
                exec(f.read(), g)
        finally:
            os.chdir(old_cwd)
            sys.argv = save_argv
            core._setup_stop_after = None
    except SystemExit:
        # Hmm, should we do something if exiting with a non-zero code
        # (ie. error)?
        pass

    return core._setup_distribution


_setup_wrapper = """\
import distutils
from distutils import core
import sys

script_name = %(script_name)s

save_argv = sys.argv.copy()
g = {"__file__": script_name, "__name__": "__main__"}
try:
    core._setup_stop_after = "init"
    sys.argv[0] = script_name
    with open(script_name, "rb") as f:
        exec(f.read(), g)
except SystemExit:
    # Hmm, should we do something if exiting with a non-zero code
    # (ie. error)?
    pass

if core._setup_distribution is None:
    raise RuntimeError(
        (
            "'distutils.core.setup()' was never called -- "
            "perhaps '%s' is not a Distutils setup script?"
        )
        % script_name
    )

d = core._setup_distribution
r = {
    'setup_requires': getattr(d, "setup_requires", []),
    'install_requires': getattr(d, "install_requires", []),
    'tests_require': getattr(d, "tests_require", []) or [],
    'scripts': getattr(d, "scripts", []) or [],
    'entry_points': getattr(d, "entry_points", None) or {},
    'packages': getattr(d, "packages", []) or [],
    'requires': d.get_requires() or [],
    }
import os
import json
with open(%(output_path)s, 'w') as f:
    json.dump(r, f)
"""


class SetupPy(BuildSystem):

    name = "setup.py"
    DEFAULT_PYTHON = 'python3'

    def __init__(self, path):
        self.path = path
        if os.path.exists(os.path.join(self.path, 'setup.py')):
            self.has_setup_py = True
        else:
            self.has_setup_py = False

        try:
            self.config = self.load_setup_cfg()
        except FileNotFoundError:
            self.config = None

        try:
            self.pyproject = self.load_toml()
        except FileNotFoundError:
            self.pyproject = None
            self.build_backend = None
        else:
            self.build_backend = self.pyproject.get("build-system", {}).get('build-backend')

    def load_toml(self):
        import toml

        with open(os.path.join(self.path, "pyproject.toml"), "r") as pf:
            return toml.load(pf)

    def load_setup_cfg(self):
        from setuptools.config import read_configuration
        return read_configuration(os.path.join(self.path, 'setup.cfg'))

    def _extract_setup(self, session=None, fixers=None):
        if session is None:
            return self._extract_setup_direct()
        else:
            return self._extract_setup_in_session(session, fixers)

    def _extract_setup_direct(self):
        p = os.path.join(self.path, 'setup.py')
        try:
            d = run_setup(os.path.abspath(p), stop_after="init")
        except RuntimeError as e:
            logging.warning("Unable to load setup.py metadata: %s", e)
            return None
        if d is None:
            logging.warning(
                "'distutils.core.setup()' was never called -- "
                "perhaps '%s' is not a Distutils setup script?" % os.path.basename(p))
            return None

        return {
            'setup_requires': getattr(d, "setup_requires", []),
            'install_requires': getattr(d, "install_requires", []),
            'tests_require': getattr(d, "tests_require", []) or [],
            'scripts': getattr(d, "scripts", []),
            'entry_points': getattr(d, "entry_points", None) or {},
            'packages': getattr(d, "packages", []),
            'requires': d.get_requires() or [],
            }

    def _extract_setup_in_session(self, session, fixers=None):
        import tempfile
        import json
        interpreter = shebang_binary(os.path.join(self.path, "setup.py"))
        if interpreter is None:
            interpreter = self.DEFAULT_PYTHON
        output_f = tempfile.NamedTemporaryFile(
            dir=os.path.join(session.location, 'tmp'), mode='w+t')
        with output_f:
            # TODO(jelmer): Perhaps run this in session, so we can install
            # missing dependencies?
            argv = [interpreter, "-c",
                    _setup_wrapper
                    .replace('%(script_name)s', '"setup.py"')
                    .replace('%(output_path)s',
                             '"/' + os.path.relpath(output_f.name, session.location) +
                             '"')]
            try:
                if fixers is not None:
                    run_with_build_fixers(session, argv, fixers)
                else:
                    session.check_call(argv, close_fds=False)
            except RuntimeError as e:
                logging.warning("Unable to load setup.py metadata: %s", e)
                return None
            output_f.seek(0)
            return json.load(output_f)

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def test(self, session, resolver, fixers):
        if self.has_setup_py:
            self._run_setup(session, resolver, ["test"], fixers)
        else:
            raise NotImplementedError

    def build(self, session, resolver, fixers):
        if self.has_setup_py:
            self._run_setup(session, resolver, ["build"], fixers)
        else:
            raise NotImplementedError

    def dist(self, session, resolver, fixers, quiet=False):
        # TODO(jelmer): Look at self.build_backend
        if self.has_setup_py:
            preargs = []
            if quiet:
                preargs.append("--quiet")
            self._run_setup(session, resolver, preargs + ["sdist"], fixers)
            return
        elif self.pyproject:
            if "poetry" in self.pyproject.get("tool", []):
                logging.debug(
                    "Found pyproject.toml with poetry section, " "assuming poetry project."
                )
                run_with_build_fixers(session, ["poetry", "build", "-f", "sdist"], fixers)
                return
        raise AssertionError("no supported section in pyproject.toml")

    def clean(self, session, resolver, fixers):
        if self.has_setup_py:
            self._run_setup(session, resolver, ["clean"], fixers)
        else:
            raise NotImplementedError

    def install(self, session, resolver, fixers, install_target):
        if self.has_setup_py:
            extra_args = []
            if install_target.user:
                extra_args.append("--user")
            self._run_setup(session, resolver, ["install"] + extra_args, fixers)
        else:
            raise NotImplementedError

    def _run_setup(self, session, resolver, args, fixers):
        from .buildlog import install_missing_reqs
        distribution = self._extract_setup(session, fixers)
        if distribution is not None:
            # Install the setup_requires beforehand, since otherwise
            # setuptools might fetch eggs instead of our preferred resolver.
            install_missing_reqs(
                session,
                resolver,
                [PythonPackageRequirement.from_requirement_str(require)
                 for require in distribution['setup_requires']])
        interpreter = shebang_binary(os.path.join(self.path, 'setup.py'))
        if interpreter is not None:
            run_with_build_fixers(session, ["./setup.py"] + args, fixers)
        else:
            # Just assume it's Python 3
            run_with_build_fixers(session, [self.DEFAULT_PYTHON, "./setup.py"] + args, fixers)

    def get_declared_dependencies(self, session, fixers=None):
        distribution = self._extract_setup(session, fixers)
        if distribution is not None:
            for require in distribution['requires']:
                yield "core", PythonPackageRequirement.from_requirement_str(require)
            # Not present for distutils-only packages
            for require in distribution['setup_requires']:
                yield "build", PythonPackageRequirement.from_requirement_str(require)
            # Not present for distutils-only packages
            for require in distribution['install_requires']:
                yield "core", PythonPackageRequirement.from_requirement_str(require)
            # Not present for distutils-only packages
            for require in distribution['tests_require']:
                yield "test", PythonPackageRequirement.from_requirement_str(require)
        if self.pyproject:
            if "build-system" in self.pyproject:
                for require in self.pyproject['build-system'].get("requires", []):
                    yield "build", PythonPackageRequirement.from_requirement_str(require)
        if self.config:
            options = self.config.get('options', {})
            for require in options.get('setup_requires', []):
                yield "build", PythonPackageRequirement.from_requirement_str(require)
            for require in options.get('install_requires', []):
                yield "core", PythonPackageRequirement.from_requirement_str(require)

    def get_declared_outputs(self, session, fixers=None):
        distribution = self._extract_setup(session, fixers)
        all_packages = set()
        if distribution is not None:
            for script in distribution['scripts']:
                yield BinaryOutput(os.path.basename(script))
            for script in distribution["entry_points"].get("console_scripts", []):
                yield BinaryOutput(script.split("=")[0])
            all_packages.update(distribution['packages'])
        if self.config:
            options = self.config.get('options', {})
            all_packages.update(options.get('packages', []))
            for script in options.get('scripts', []):
                yield BinaryOutput(os.path.basename(script))
            for script in options.get("entry_points", {}).get("console_scripts", []):
                yield BinaryOutput(script.split("=")[0])

        packages = set()
        for package in sorted(all_packages):
            pts = package.split('.')
            b = []
            for e in pts:
                b.append(e)
                if '.'.join(b) in packages:
                    break
            else:
                packages.add(package)
        for package in packages:
            yield PythonPackageOutput(package, python_version="cpython3")

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "setup.py")):
            logging.debug("Found setup.py, assuming python project.")
            return cls(path)
        if os.path.exists(os.path.join(path, "setup.cfg")):
            logging.debug("Found setup.py, assuming python project.")
            return cls(path)
        if os.path.exists(os.path.join(path, "pyproject.toml")):
            logging.debug("Found pyproject.toml, assuming python project.")
            return cls(path)


class Gradle(BuildSystem):

    name = "gradle"

    def __init__(self, path, executable="gradle"):
        self.path = path
        self.executable = executable

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    @classmethod
    def exists(cls, path):
        return (
            os.path.exists(os.path.join(path, "build.gradle")) or
            os.path.exists(os.path.join(path, "build.gradle.kts")))

    @classmethod
    def from_path(cls, path):
        if os.path.exists(os.path.join(path, "gradlew")):
            return cls(path, "./gradlew")
        return cls(path)

    @classmethod
    def probe(cls, path):
        if cls.exists(path):
            logging.debug("Found build.gradle, assuming gradle package.")
            return cls.from_path(path)

    def setup(self, resolver):
        if not self.executable.startswith('./'):
            resolver.install([BinaryRequirement(self.executable)])

    def clean(self, session, resolver, fixers):
        self.setup(resolver)
        run_with_build_fixers(session, [self.executable, "clean"], fixers)

    def build(self, session, resolver, fixers):
        self.setup(resolver)
        run_with_build_fixers(session, [self.executable, "build"], fixers)

    def test(self, session, resolver, fixers):
        self.setup(resolver)
        run_with_build_fixers(session, [self.executable, "test"], fixers)

    def dist(self, session, resolver, fixers, quiet=False):
        self.setup(resolver)
        run_with_build_fixers(session, [self.executable, "distTar"], fixers)

    def install(self, session, resolver, fixers, install_target):
        raise NotImplementedError
        self.setup(resolver)
        # TODO(jelmer): installDist just creates files under build/install/...
        run_with_build_fixers(
            session, [self.executable, "installDist"], fixers)


class R(BuildSystem):

    # https://r-pkgs.org/description.html

    name = "R"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def build(self, session, resolver, fixers):
        pass

    def dist(self, session, resolver, fixers, quiet=False):
        run_with_build_fixers(session, ["R", "CMD", "build", "."], fixers)

    def install(self, session, resolver, fixers, install_target):
        run_with_build_fixers(session, ["R", "CMD", "INSTALL", "."], fixers)

    def test(self, session, resolver, fixers):
        run_with_build_fixers(session, ["R", "CMD", "check", "."], fixers)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, 'DESCRIPTION')):
            return cls(path)

    def _read_description(self):
        path = os.path.join(self.path, 'DESCRIPTION')
        from email.parser import BytesParser
        with open(path, 'rb') as f:
            return BytesParser().parse(f)

    def get_declared_dependencies(self, session, fixers=None):
        def parse_list(t):
            return [s.strip() for s in t.split(',') if s.strip()]
        description = self._read_description()
        if 'Suggests' in description:
            for s in parse_list(description['Suggests']):
                yield "build", RPackageRequirement.from_str(s)
        if 'Depends' in description:
            for s in parse_list(description['Depends']):
                yield "build", RPackageRequirement.from_str(s)
        if 'Imports' in description:
            for s in parse_list(description['Imports']):
                yield "build", RPackageRequirement.from_str(s)

    def get_declared_outputs(self, session, fixers=None):
        description = self._read_description()
        if 'Package' in description:
            yield RPackageOutput(description['Package'])


class Meson(BuildSystem):

    name = "meson"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def _setup(self, session, fixers):
        if session.exists("build"):
            return
        session.check_call(['mkdir', 'build'])
        run_with_build_fixers(session, ["meson", "setup", "build"], fixers)

    def clean(self, session, resolver, fixers):
        self._setup(session, fixers)
        run_with_build_fixers(session, ["ninja", "-C", "build", "clean"], fixers)

    def build(self, session, resolver, fixers):
        self._setup(session, fixers)
        run_with_build_fixers(session, ["ninja", "-C", "build"], fixers)

    def dist(self, session, resolver, fixers, quiet=False):
        self._setup(session, fixers)
        run_with_build_fixers(session, ["ninja", "-C", "build", "dist"], fixers)

    def test(self, session, resolver, fixers):
        self._setup(session, fixers)
        run_with_build_fixers(session, ["ninja", "-C", "build", "test"], fixers)

    def install(self, session, resolver, fixers, install_target):
        self._setup(session, fixers)
        run_with_build_fixers(session, ["ninja", "-C", "build", "install"], fixers)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "meson.build")):
            logging.debug("Found meson.build, assuming meson package.")
            return Meson(os.path.join(path, "meson.build"))


class Npm(BuildSystem):

    name = "npm"

    def __init__(self, path):
        import json

        with open(path, "r") as f:
            self.package = json.load(f)

    def get_declared_dependencies(self, session, fixers=None):
        if "devDependencies" in self.package:
            for name, unused_version in self.package["devDependencies"].items():
                # TODO(jelmer): Look at version
                yield "dev", NodePackageRequirement(name)

    def setup(self, resolver):
        resolver.install([BinaryRequirement("npm")])

    def dist(self, session, resolver, fixers, quiet=False):
        self.setup(resolver)
        run_with_build_fixers(session, ["npm", "pack"], fixers)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "package.json")):
            logging.debug("Found package.json, assuming node package.")
            return cls(os.path.join(path, "package.json"))


class Waf(BuildSystem):

    name = "waf"

    def __init__(self, path):
        self.path = path

    def setup(self, session, resolver, fixers):
        resolver.install([BinaryRequirement("python3")])

    def dist(self, session, resolver, fixers, quiet=False):
        self.setup(session, resolver, fixers)
        run_with_build_fixers(session, ["./waf", "dist"], fixers)

    def test(self, session, resolver, fixers):
        self.setup(session, resolver, fixers)
        run_with_build_fixers(session, ["./waf", "test"], fixers)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "waf")):
            logging.debug("Found waf, assuming waf package.")
            return cls(os.path.join(path, "waf"))


class Gem(BuildSystem):

    name = "gem"

    def __init__(self, path):
        self.path = path

    def setup(self, resolver):
        resolver.install([BinaryRequirement("gem2deb")])

    def dist(self, session, resolver, fixers, quiet=False):
        self.setup(resolver)
        gemfiles = [
            entry.name for entry in session.scandir(".") if entry.name.endswith(".gem")
        ]
        if len(gemfiles) > 1:
            logging.warning("More than one gemfile. Trying the first?")
        run_with_build_fixers(session, ["gem2tgz", gemfiles[0]], fixers)

    @classmethod
    def probe(cls, path):
        gemfiles = [entry.path for entry in os.scandir(path) if entry.name.endswith(".gem")]
        if gemfiles:
            return cls(gemfiles[0])


class DistInkt(BuildSystem):
    def __init__(self, path):
        self.path = path
        self.name = "dist-zilla"
        self.dist_inkt_class = None
        with open(self.path, "rb") as f:
            for line in f:
                if not line.startswith(b";;"):
                    continue
                try:
                    (key, value) = line[2:].split(b"=", 1)
                except ValueError:
                    continue
                if key.strip() == b"class" and value.strip().startswith(b"'Dist::Inkt"):
                    logging.debug(
                        "Found Dist::Inkt section in dist.ini, " "assuming distinkt."
                    )
                    self.name = "dist-inkt"
                    self.dist_inkt_class = value.decode().strip("'")
                    return
        logging.debug("Found dist.ini, assuming dist-zilla.")

    def setup(self, resolver):
        resolver.install(
            [
                PerlModuleRequirement("Dist::Inkt"),
            ]
        )

    def dist(self, session, resolver, fixers, quiet=False):
        self.setup(resolver)
        if self.name == "dist-inkt":
            resolver.install([PerlModuleRequirement(self.dist_inkt_class)])
            run_with_build_fixers(session, ["distinkt-dist"], fixers)
        else:
            # Default to invoking Dist::Zilla
            resolver.install([PerlModuleRequirement("Dist::Zilla")])
            run_with_build_fixers(session, ["dzil", "build", "--in", ".."], fixers)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "dist.ini")) and not os.path.exists(
            os.path.join(path, "Makefile.PL")
        ):
            return cls(os.path.join(path, "dist.ini"))

    def get_declared_dependencies(self, session, fixers=None):
        out = session.check_output(["dzil", "authordeps"])
        for entry in out.splitlines():
            yield "build", PerlModuleRequirement(entry.decode())


class Make(BuildSystem):

    name = "make"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def setup(self, session, resolver, fixers):
        def makefile_exists():
            return any(
                [session.exists(p) for p in ["Makefile", "GNUmakefile", "makefile"]]
            )

        if session.exists("Makefile.PL") and not makefile_exists():
            run_with_build_fixers(session, ["perl", "Makefile.PL"], fixers)

        if not makefile_exists() and not session.exists("configure"):
            if session.exists("autogen.sh"):
                if shebang_binary(os.path.join(self.path, "autogen.sh")) is None:
                    run_with_build_fixers(session, ["/bin/sh", "./autogen.sh"], fixers)
                try:
                    run_with_build_fixers(session, ["./autogen.sh"], fixers)
                except UnidentifiedError as e:
                    if (
                        "Gnulib not yet bootstrapped; "
                        "run ./bootstrap instead." in e.lines
                    ):
                        run_with_build_fixers(session, ["./bootstrap"], fixers)
                        run_with_build_fixers(session, ["./autogen.sh"], fixers)
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

        if not makefile_exists() and session.exists("configure"):
            run_with_build_fixers(session, ["./configure"], fixers)

        if not makefile_exists() and any([n.name.endswith('.pro') for n in session.scandir(".")]):
            run_with_build_fixers(session, ["qmake"], fixers)

    def build(self, session, resolver, fixers):
        self.setup(session, resolver, fixers)
        run_with_build_fixers(session, ["make", "all"], fixers)

    def clean(self, session, resolver, fixers):
        self.setup(session, resolver, fixers)
        run_with_build_fixers(session, ["make", "clean"], fixers)

    def test(self, session, resolver, fixers):
        self.setup(session, resolver, fixers)
        run_with_build_fixers(session, ["make", "check"], fixers)

    def install(self, session, resolver, fixers, install_target):
        self.setup(session, resolver, fixers)
        run_with_build_fixers(session, ["make", "install"], fixers)

    def dist(self, session, resolver, fixers, quiet=False):
        self.setup(session, resolver, fixers)
        try:
            run_with_build_fixers(session, ["make", "dist"], fixers)
        except UnidentifiedError as e:
            if "make: *** No rule to make target 'dist'.  Stop." in e.lines:
                raise NotImplementedError
            elif "make[1]: *** No rule to make target 'dist'. Stop." in e.lines:
                raise NotImplementedError
            elif (
                "Reconfigure the source tree "
                "(via './config' or 'perl Configure'), please."
            ) in e.lines:
                run_with_build_fixers(session, ["./config"], fixers)
                run_with_build_fixers(session, ["make", "dist"], fixers)
            elif (
                "Please try running 'make manifest' and then run "
                "'make dist' again." in e.lines
            ):
                run_with_build_fixers(session, ["make", "manifest"], fixers)
                run_with_build_fixers(session, ["make", "dist"], fixers)
            elif "Please run ./configure first" in e.lines:
                run_with_build_fixers(session, ["./configure"], fixers)
                run_with_build_fixers(session, ["make", "dist"], fixers)
            elif any(
                [
                    re.match(
                        r"(Makefile|GNUmakefile|makefile):[0-9]+: "
                        r"\*\*\* Missing \'Make.inc\' "
                        r"Run \'./configure \[options\]\' and retry.  Stop.",
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

    def get_declared_dependencies(self, session, fixers=None):
        # TODO(jelmer): Split out the perl-specific stuff?
        if os.path.exists(os.path.join(self.path, "META.yml")):
            # See http://module-build.sourceforge.net/META-spec-v1.4.html for
            # the specification of the format.
            import ruamel.yaml
            import ruamel.yaml.reader

            with open(os.path.join(self.path, "META.yml"), "rb") as f:
                try:
                    data = ruamel.yaml.load(f, ruamel.yaml.SafeLoader)
                except ruamel.yaml.reader.ReaderError as e:
                    warnings.warn("Unable to parse META.yml: %s" % e)
                    return
                for require in data.get("requires", []):
                    yield "build", PerlModuleRequirement(require)
        else:
            raise NotImplementedError

    @classmethod
    def probe(cls, path):
        if any(
            [
                os.path.exists(os.path.join(path, p))
                for p in [
                    "Makefile",
                    "GNUmakefile",
                    "makefile",
                    "Makefile.PL",
                    "CMakeLists.txt",
                    "autogen.sh",
                    "configure.ac",
                    "configure.in",
                ]
            ]
        ):
            return cls(path)
        for n in os.scandir(path):
            # qmake
            if n.name.endswith('.pro'):
                return cls(path)


class Cargo(BuildSystem):

    name = "cargo"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def __init__(self, path):
        from toml.decoder import load

        self.path = path

        with open(path, "r") as f:
            self.cargo = load(f)

    def get_declared_dependencies(self, session, fixers=None):
        if "dependencies" in self.cargo:
            for name, details in self.cargo["dependencies"].items():
                if isinstance(details, str):
                    details = {"version": details}
                # TODO(jelmer): Look at details['version']
                yield "build", CargoCrateRequirement(
                    name,
                    features=details.get('features', []),
                    version=details.get("version"))

    def test(self, session, resolver, fixers):
        run_with_build_fixers(session, ["cargo", "test"], fixers)

    def clean(self, session, resolver, fixers):
        run_with_build_fixers(session, ["cargo", "clean"], fixers)

    def build(self, session, resolver, fixers):
        run_with_build_fixers(session, ["cargo", "build"], fixers)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "Cargo.toml")):
            logging.debug("Found Cargo.toml, assuming rust cargo package.")
            return Cargo(os.path.join(path, "Cargo.toml"))


class Golang(BuildSystem):
    """Go builds."""

    name = "golang"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s()" % (type(self).__name__)

    def test(self, session, resolver, fixers):
        run_with_build_fixers(session, ["go", "test"], fixers)

    def build(self, session, resolver, fixers):
        run_with_build_fixers(session, ["go", "build"], fixers)

    def install(self, session, resolver, fixers):
        run_with_build_fixers(session, ["go", "install"], fixers)

    def clean(self, session, resolver, fixers):
        session.check_call(["go", "clean"])

    @classmethod
    def probe(cls, path):
        for entry in os.scandir(path):
            if entry.name.endswith(".go"):
                return Golang(path)
            if entry.is_dir():
                for entry in os.scandir(entry.path):
                    if entry.name.endswith(".go"):
                        return Golang(path)


class Maven(BuildSystem):

    name = "maven"

    def __init__(self, path):
        self.path = path

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "pom.xml")):
            logging.debug("Found pom.xml, assuming maven package.")
            return cls(os.path.join(path, "pom.xml"))


class Cabal(BuildSystem):

    name = "cabal"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def _run(self, session, args, fixers):
        try:
            run_with_build_fixers(session, ["runhaskell", "Setup.hs"] + args, fixers)
        except UnidentifiedError as e:
            if "Run the 'configure' command first." in e.lines:
                run_with_build_fixers(
                    session, ["runhaskell", "Setup.hs", "configure"], fixers
                )
                run_with_build_fixers(
                    session, ["runhaskell", "Setup.hs"] + args, fixers
                )
            else:
                raise

    def test(self, session, resolver, fixers):
        self._run(session, ["test"], fixers)

    def dist(self, session, resolver, fixers, quiet=False):
        self._run(session, ["sdist"], fixers)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "Setup.hs")):
            logging.debug("Found Setup.hs, assuming haskell package.")
            return cls(os.path.join(path, "Setup.hs"))


class PerlBuildTiny(BuildSystem):

    name = "perl-build-tiny"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def setup(self, session, fixers):
        run_with_build_fixers(session, ["perl", "Build.PL"], fixers)

    def test(self, session, resolver, fixers):
        self.setup(session, fixers)
        run_with_build_fixers(session, ["./Build", "test"], fixers)

    def build(self, session, resolver, fixers):
        self.setup(session, fixers)
        run_with_build_fixers(session, ["./Build", "build"], fixers)

    def clean(self, session, resolver, fixers):
        self.setup(session, fixers)
        run_with_build_fixers(session, ["./Build", "clean"], fixers)

    def install(self, session, resolver, fixers, install_target):
        self.setup(session, fixers)
        run_with_build_fixers(session, ["./Build", "install"], fixers)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "Build.PL")):
            logging.debug("Found Build.PL, assuming Module::Build::Tiny package.")
            return cls(path)


BUILDSYSTEM_CLSES = [
    Pear, SetupPy, Npm, Waf, Cargo, Meson, Cabal, Gradle, Maven,
    DistInkt, Gem, Make, PerlBuildTiny, Golang, R]


def scan_buildsystems(path):
    """Detect build systems."""
    ret = []
    ret.extend([('.', bs) for bs in detect_buildsystems(path)])

    if not ret:
        # Nothing found. Try the next level?
        for entry in os.scandir(path):
            if entry.is_dir():
                ret.extend(
                    [(entry.name, bs) for bs in detect_buildsystems(entry.path)])

    return ret


def detect_buildsystems(path):
    for bs_cls in BUILDSYSTEM_CLSES:
        bs = bs_cls.probe(path)
        if bs is not None:
            yield bs


def get_buildsystem(path: str) -> Tuple[str, BuildSystem]:
    for subpath, buildsystem in scan_buildsystems(path):
        return subpath, buildsystem

    raise NoBuildToolsFound()

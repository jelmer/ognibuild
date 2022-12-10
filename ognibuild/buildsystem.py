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


from contextlib import suppress
import logging
import os
import re
from typing import Optional, Tuple, Type, List, Iterable
import warnings

from . import shebang_binary, UnidentifiedError
from .dist_catcher import DistCatcher
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
    OctavePackageRequirement,
    PhpPackageRequirement,
    MavenArtifactRequirement,
    GoRequirement,
    GoPackageRequirement,
    VagueDependencyRequirement,
)
from .fix_build import run_with_build_fixers, run_detecting_problems
from .session import which


def guaranteed_which(session, resolver, name):
    path = which(session, name)
    if not path:
        resolver.install([BinaryRequirement(name)])
    return which(session, name)


class NoBuildToolsFound(Exception):
    """No supported build tools were found."""


class InstallTarget:  # noqa: PIE793

    # Whether to prefer user-specific installation
    user: Optional[bool]

    prefix: Optional[str]

    # TODO(jelmer): Add information about target directory, layout, etc.


def get_necessary_declared_requirements(resolver, requirements, stages):
    missing = []
    for stage, req in requirements:
        if stage in stages:
            missing.append(req)
    return missing


class BuildSystem:
    """A particular buildsystem."""

    name: str

    def __str__(self):
        return self.name

    def dist(
        self, session, resolver, target_directory: str, quiet=False
    ) -> str:
        raise NotImplementedError(self.dist)

    def install_declared_requirements(self, stages, session, resolver, fixers):
        from .buildlog import install_missing_reqs
        declared_reqs = self.get_declared_dependencies(session, fixers)
        relevant = get_necessary_declared_requirements(
            resolver, declared_reqs, stages)
        install_missing_reqs(session, resolver, relevant, explain=False)

    def test(self, session, resolver):
        raise NotImplementedError(self.test)

    def build(self, session, resolver):
        raise NotImplementedError(self.build)

    def clean(self, session, resolver):
        raise NotImplementedError(self.clean)

    def install(self, session, resolver, install_target):
        raise NotImplementedError(self.install)

    def get_declared_dependencies(self, session, fixers=None):
        raise NotImplementedError(self.get_declared_dependencies)

    def get_declared_outputs(self, session, fixers=None):
        raise NotImplementedError(self.get_declared_outputs)

    @classmethod
    def probe(cls, path: str) -> Optional["BuildSystem"]:
        return None


def xmlparse_simplify_namespaces(path, namespaces):
    import xml.etree.ElementTree as ET

    namespaces = ["{%s}" % ns for ns in namespaces]
    tree = ET.iterparse(path)
    for _, el in tree:
        for namespace in namespaces:
            el.tag = el.tag.replace(namespace, "")
    return tree.root  # type: ignore


class Pear(BuildSystem):

    name = "pear"

    PEAR_NAMESPACES = [
        "http://pear.php.net/dtd/package-2.0",
        "http://pear.php.net/dtd/package-2.1",
        ]

    def __init__(self, path):
        self.path = path

    def dist(self, session, resolver, target_directory: str,
             quiet: bool = False):
        with DistCatcher([session.external_path(".")]) as dc:
            run_detecting_problems(
                session,
                [guaranteed_which(session, resolver, "pear"), "package"])
        return dc.copy_single(target_directory)

    def test(self, session, resolver):
        run_detecting_problems(
            session,
            [guaranteed_which(session, resolver, "pear"), "run-tests"])

    def build(self, session, resolver):
        run_detecting_problems(
            session,
            [guaranteed_which(session, resolver, "pear"), "build", self.path])

    def clean(self, session, resolver):
        pass  # TODO

    def install(self, session, resolver, install_target):
        run_detecting_problems(
            session,
            [guaranteed_which(
                session, resolver, "pear"), "install", self.path])

    def get_declared_dependencies(self, session, fixers=None):
        path = os.path.join(self.path, "package.xml")
        import xml.etree.ElementTree as ET

        try:
            root = xmlparse_simplify_namespaces(
                path,
                self.PEAR_NAMESPACES
            )
        except ET.ParseError as e:
            logging.warning("Unable to parse package.xml: %s", e)
            return
        assert root.tag == "package", "root tag is %r" % root.tag
        dependencies_tag = root.find("dependencies")
        if dependencies_tag is not None:
            required_tag = root.find("dependencies")
            if required_tag is not None:
                for package_tag in root.findall("package"):
                    name = package_tag.find("name").text
                    min_tag = package_tag.find("min")
                    max_tag = package_tag.find("max")
                    channel_tag = package_tag.find("channel")
                    yield "core", PhpPackageRequirement(
                        name,
                        channel=(channel_tag.text if channel_tag else None),
                        min_version=(min_tag.text if min_tag else None),
                        max_version=(max_tag.text if max_tag else None),
                    )

    @classmethod
    def probe(cls, path):
        package_xml_path = os.path.join(path, "package.xml")
        if not os.path.exists(package_xml_path):
            return

        import xml.etree.ElementTree as ET
        try:
            tree = ET.iterparse(package_xml_path)
        except ET.ParseError as e:
            logging.warning("Unable to parse package.xml: %s", e)
            return

        if not tree.root:  # type: ignore
            # No root?
            return

        for ns in cls.PEAR_NAMESPACES:
            if tree.root.tag == '{%s}package' % ns:  # type: ignore
                logging.debug(
                    "Found package.xml with namespace %s, "
                    "assuming pear package.")
                return cls(path)


# run_setup, but setting __name__
# Imported from Python's distutils.core, Copyright (C) PSF


def run_setup(script_name, script_args=None, stop_after="run"):
    # Import setuptools, just in case it decides to replace distutils
    with suppress(ImportError):
        import setuptools  # noqa: F401
    from distutils import core
    import sys

    if stop_after not in ("init", "config", "commandline", "run"):
        raise ValueError("invalid value for 'stop_after': %r" % (stop_after,))

    core._setup_stop_after = stop_after  # type: ignore

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
            core._setup_stop_after = None  # type: ignore
    except SystemExit:
        # Hmm, should we do something if exiting with a non-zero code
        # (ie. error)?
        pass

    return core._setup_distribution  # type: ignore


_setup_wrapper = """\
try:
    import setuptools
except ImportError:
    pass
import distutils
from distutils import core
import sys

script_name = %(script_name)s

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
    'name': d.name,
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
    DEFAULT_PYTHON = "python3"

    def __init__(self, path):
        self.path = path
        if os.path.exists(os.path.join(self.path, "setup.py")):
            self.has_setup_py = True
        else:
            self.has_setup_py = False

        try:
            self.config = self.load_setup_cfg()
        except FileNotFoundError:
            self.config = None
        except ModuleNotFoundError as e:
            logging.warning('Error parsing setup.cfg: %s', e)
            self.config = None

        try:
            self.pyproject = self.load_toml()
        except FileNotFoundError:
            self.pyproject = None
            self.build_backend = None
        else:
            self.build_backend = self.pyproject.get("build-system", {}).get(
                "build-backend"
            )

    def load_toml(self):
        import toml

        with open(os.path.join(self.path, "pyproject.toml"), "r") as pf:
            return toml.load(pf)

    def load_setup_cfg(self):
        from setuptools.config.setupcfg import read_configuration

        p = os.path.join(self.path, "setup.cfg")
        if os.path.exists(p):
            return read_configuration(p)
        raise FileNotFoundError(p)

    def _extract_setup(self, session=None, fixers=None):
        if not self.has_setup_py:
            return None
        if session is None:
            return self._extract_setup_direct()
        else:
            return self._extract_setup_in_session(session, fixers)

    def _extract_setup_direct(self):
        p = os.path.join(self.path, "setup.py")
        try:
            d = run_setup(os.path.abspath(p), stop_after="init")
        except RuntimeError as e:
            logging.warning("Unable to load setup.py metadata: %s", e)
            return None
        if d is None:
            logging.warning(
                "'distutils.core.setup()' was never called -- "
                "perhaps '%s' is not a Distutils setup script?",
                os.path.basename(p)
            )
            return None

        return {
            "name": d.name,
            "setup_requires": getattr(d, "setup_requires", []),
            "install_requires": getattr(d, "install_requires", []),
            "tests_require": getattr(d, "tests_require", []) or [],
            "scripts": getattr(d, "scripts", []),
            "entry_points": getattr(d, "entry_points", None) or {},
            "packages": getattr(d, "packages", []),
            "requires": d.get_requires() or [],
        }

    def _extract_setup_in_session(self, session, fixers=None):
        import tempfile
        import json

        interpreter = self._determine_interpreter()
        output_f = tempfile.NamedTemporaryFile(
            dir=os.path.join(session.location, "tmp"), mode="w+t"
        )
        with output_f:
            # TODO(jelmer): Perhaps run this in session, so we can install
            # missing dependencies?
            argv = [
                interpreter,
                "-c",
                _setup_wrapper.replace(
                    "%(script_name)s", '"setup.py"').replace(
                        "%(output_path)s",
                        '"/' + os.path.relpath(output_f.name, session.location)
                        + '"',
                ),
            ]
            try:
                if fixers is not None:
                    run_with_build_fixers(fixers, session, argv, quiet=True)
                else:
                    session.check_call(argv, close_fds=False)
            except RuntimeError as e:
                logging.warning("Unable to load setup.py metadata: %s", e)
                return None
            output_f.seek(0)
            return json.load(output_f)

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def test(self, session, resolver):
        if os.path.exists(os.path.join(self.path, "tox.ini")):
            run_detecting_problems(
                session, ["tox", "--skip-missing-interpreters"])
            return
        if self.config and (
                'tool:pytest' in self.config or 'pytest' in self.config):
            run_detecting_problems(session, ['pytest'])
            return
        if self.has_setup_py:
            # Pre-emptively insall setuptools, since distutils doesn't provide
            # a 'test' subcommand and some packages fall back to distutils
            # if setuptools is not available.
            setuptools_req = PythonPackageRequirement("setuptools")
            if not setuptools_req.met(session):
                resolver.install([setuptools_req])
            try:
                self._run_setup(session, resolver, ["test"])
            except UnidentifiedError as e:
                if "error: invalid command 'test'" in e.lines:
                    pass
                else:
                    raise
            else:
                return
        raise NotImplementedError

    def build(self, session, resolver):
        if self.has_setup_py:
            self._run_setup(session, resolver, ["build"])
        else:
            raise NotImplementedError

    def dist(self, session, resolver, target_directory, quiet=False):
        # TODO(jelmer): Look at self.build_backend
        if self.has_setup_py:
            preargs = []
            if quiet:
                preargs.append("--quiet")
            # Preemptively install setuptools since some packages fail in
            # some way without it.
            setuptools_req = PythonPackageRequirement("setuptools")
            if not setuptools_req.met(session):
                resolver.install([setuptools_req])
            with DistCatcher([session.external_path("dist")]) as dc:
                self._run_setup(session, resolver, preargs + ["sdist"])
            return dc.copy_single(target_directory)
        elif self.pyproject:
            with DistCatcher([session.external_path("dist")]) as dc:
                run_detecting_problems(
                    session,
                    ["python3", "-m", "build", "--sdist", "."],
                )
            return dc.copy_single(target_directory)
        raise AssertionError("no setup.py or pyproject.toml")

    def clean(self, session, resolver):
        if self.has_setup_py:
            self._run_setup(session, resolver, ["clean"])
        else:
            raise NotImplementedError

    def install(self, session, resolver, install_target):
        if self.has_setup_py:
            extra_args = []
            if install_target.user:
                extra_args.append("--user")
            if install_target.prefix:
                extra_args.append("--prefix=%s" % install_target.prefix)
            self._run_setup(
                session, resolver, ["install"] + extra_args)
        else:
            raise NotImplementedError

    def _determine_interpreter(self):
        interpreter = None
        if self.config:
            python_requires = self.config.get(
                'options', {}).get('python_requires')
            if python_requires and not python_requires.contains('2.7'):
                interpreter = 'python3'
        if interpreter is None:
            interpreter = shebang_binary(os.path.join(self.path, "setup.py"))
        if interpreter is None:
            interpreter = self.DEFAULT_PYTHON
        return interpreter

    def _run_setup(self, session, resolver, args):
        from .buildlog import install_missing_reqs

        # Install the setup_requires beforehand, since otherwise
        # setuptools might fetch eggs instead of our preferred resolver.
        install_missing_reqs(session, resolver, list(self._setup_requires()))
        interpreter = self._determine_interpreter()
        argv = [interpreter, "./setup.py"] + args
        # TODO(jelmer): Perhaps this should be additive?
        env = dict(os.environ)
        run_detecting_problems(session, argv, env=env)

    def _setup_requires(self):
        if self.pyproject:  # noqa: SIM102
            if "build-system" in self.pyproject:
                requires = self.pyproject["build-system"].get("requires", [])
                for require in requires:
                    yield PythonPackageRequirement.from_requirement_str(
                        require)
        if self.config:
            options = self.config.get("options", {})
            for require in options.get("setup_requires", []):
                yield PythonPackageRequirement.from_requirement_str(require)

    def get_declared_dependencies(self, session, fixers=None):
        distribution = self._extract_setup(session, fixers)
        if distribution is not None:
            for require in distribution["requires"]:
                yield "core", PythonPackageRequirement.from_requirement_str(
                    require)
            # Not present for distutils-only packages
            for require in distribution["setup_requires"]:
                yield "build", PythonPackageRequirement.from_requirement_str(
                    require)
            # Not present for distutils-only packages
            for require in distribution["install_requires"]:
                yield "core", PythonPackageRequirement.from_requirement_str(
                    require)
            # Not present for distutils-only packages
            for require in distribution["tests_require"]:
                yield "test", PythonPackageRequirement.from_requirement_str(
                    require)
        if self.pyproject:  # noqa: SIM102
            if "build-system" in self.pyproject:
                requires = self.pyproject["build-system"].get("requires", [])
                for require in requires:
                    yield (
                        "build",
                        PythonPackageRequirement.from_requirement_str(require))
        if self.config:
            options = self.config.get("options", {})
            for require in options.get("setup_requires", []):
                yield "build", PythonPackageRequirement.from_requirement_str(
                    require)
            for require in options.get("install_requires", []):
                yield "core", PythonPackageRequirement.from_requirement_str(
                    require)

    def get_declared_outputs(self, session, fixers=None):
        distribution = self._extract_setup(session, fixers)
        all_packages = set()
        if distribution is not None:
            for script in distribution["scripts"]:
                yield BinaryOutput(os.path.basename(script))
            for script in distribution["entry_points"].get(
                    "console_scripts", []):
                yield BinaryOutput(script.split("=")[0])
            all_packages.update(distribution["packages"])
        if self.config:
            options = self.config.get("options", {})
            all_packages.update(options.get("packages", []))
            for script in options.get("scripts", []):
                yield BinaryOutput(os.path.basename(script))
            for script in options.get("entry_points", {}).get(
                    "console_scripts", []):
                yield BinaryOutput(script.split("=")[0])

        packages = set()
        for package in sorted(all_packages):
            pts = package.split(".")
            b = []
            for e in pts:
                b.append(e)
                if ".".join(b) in packages:
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
        if os.path.exists(os.path.join(path, "pyproject.toml")):
            logging.debug("Found pyproject.toml, assuming python project.")
            return cls(path)


class Bazel(BuildSystem):

    name = "bazel"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    @classmethod
    def exists(cls, path):
        return os.path.exists(os.path.join(path, "BUILD"))

    @classmethod
    def probe(cls, path):
        if cls.exists(path):
            logging.debug("Found BUILD, assuming bazel package.")
            return cls(path)

    def build(self, session, resolver):
        run_detecting_problems(session, ["bazel", "build", "//..."])

    def test(self, session, resolver):
        run_detecting_problems(session, ["bazel", "test", "//..."])


class GnomeShellExtension(BuildSystem):

    name = "gnome-shell-extension"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    @classmethod
    def exists(cls, path):
        return os.path.exists(os.path.join(path, "metadata.json"))

    @classmethod
    def probe(cls, path):
        if cls.exists(path):
            logging.debug(
                "Found metadata.json , assuming gnome-shell extension.")
            return cls(path)

    def build(self, session, resolver):
        pass

    def test(self, session, resolver):
        pass

    def get_declared_dependencies(self, session, fixers=None):
        import json
        with open(os.path.join(self.path, 'metadata.json'), 'r') as f:
            metadata = json.load(f)
        if 'shell-version' in metadata:
            # TODO(jelmer): Somehow represent supported versions
            yield "core", VagueDependencyRequirement("gnome-shell")


class Octave(BuildSystem):

    name = "octave"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    @classmethod
    def exists(cls, path):
        if not os.path.exists(os.path.join(path, "DESCRIPTION")):
            return False
        # Urgh, isn't there a better way to see if this is an octave package?
        for entry in os.scandir(path):
            if entry.name.endswith(".m"):
                return True
            if not entry.is_dir():
                continue
            for subentry in os.scandir(entry.path):
                if subentry.name.endswith(".m"):
                    return True
        return False

    @classmethod
    def probe(cls, path):
        if cls.exists(path):
            logging.debug("Found DESCRIPTION, assuming octave package.")
            return cls(path)

    def _read_description(self):
        path = os.path.join(self.path, "DESCRIPTION")
        from email.parser import BytesParser

        with open(path, "rb") as f:
            return BytesParser().parse(f)

    def get_declared_dependencies(self, session, fixers=None):
        def parse_list(t):
            return [s.strip() for s in t.split(",") if s.strip()]

        description = self._read_description()
        if "Depends" in description:
            for s in parse_list(description["Depends"]):
                yield "build", OctavePackageRequirement.from_str(s)


class Gradle(BuildSystem):

    name = "gradle"

    def __init__(self, path, executable="gradle"):
        self.path = path
        self.executable = executable

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    @classmethod
    def exists(cls, path):
        return (os.path.exists(os.path.join(path, "build.gradle"))
                or os.path.exists(os.path.join(path, "build.gradle.kts")))

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

    def setup(self, session, resolver):
        if not self.executable.startswith("./"):
            binary_req = BinaryRequirement(self.executable)
            if not binary_req.met(session):
                resolver.install([binary_req])

    def _run(self, session, resolver, task, args):
        self.setup(session, resolver)
        argv = []
        if self.executable.startswith("./") and (
            not os.access(os.path.join(self.path, self.executable), os.X_OK)
        ):
            argv.append("sh")
        argv.extend([self.executable, task])
        argv.extend(args)
        try:
            run_detecting_problems(session, argv)
        except UnidentifiedError as e:
            if any(
                    re.match(
                        r"Task '" + task +
                        r"' not found in root project '.*'\.", line
                    )
                    for line in e.lines
            ):
                raise NotImplementedError from e
            raise

    def clean(self, session, resolver):
        self._run(session, resolver, "clean", [])

    def build(self, session, resolver):
        self._run(session, resolver, "build", [])

    def test(self, session, resolver):
        self._run(session, resolver, "test", [])

    def dist(self, session, resolver, target_directory, quiet=False):
        with DistCatcher([session.external_path(".")]) as dc:
            self._run(session, resolver, "distTar", [])
        return dc.copy_single(target_directory)

    def install(self, session, resolver, install_target):
        raise NotImplementedError
        # TODO(jelmer): installDist just creates files under build/install/...
        self._run(session, resolver, "installDist", [])


class R(BuildSystem):

    # https://r-pkgs.org/description.html

    name = "R"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def build(self, session, resolver):
        pass

    def dist(self, session, resolver, target_directory, quiet=False):
        r_path = guaranteed_which(session, resolver, "R")
        with DistCatcher([session.external_path(".")]) as dc:
            run_detecting_problems(session, [r_path, "CMD", "build", "."])
        return dc.copy_single(target_directory)

    def install(self, session, resolver, install_target):
        extra_args = []
        if install_target.prefix:
            extra_args.append("--prefix=%s" % install_target.prefix)
        r_path = guaranteed_which(session, resolver, "R")
        run_detecting_problems(
            session, [r_path, "CMD", "INSTALL", "."] + extra_args)

    def test(self, session, resolver):
        r_path = guaranteed_which(session, resolver, "R")
        if session.exists("run_tests.sh"):
            run_detecting_problems(session, ["./run_tests.sh"])
        elif session.exists("tests/testthat"):
            run_detecting_problems(
                session, [r_path, "-e", "testthat::test_dir('tests')"])

    def lint(self, session, resolver):
        r_path = guaranteed_which(session, resolver, "R")
        run_detecting_problems(session, [r_path, "CMD", "check"])

    @classmethod
    def probe(cls, path):
        if (os.path.exists(os.path.join(path, "DESCRIPTION"))
                and os.path.exists(os.path.join(path, "NAMESPACE"))):
            return cls(path)

    def _read_description(self):
        path = os.path.join(self.path, "DESCRIPTION")
        from email.parser import BytesParser

        with open(path, "rb") as f:
            return BytesParser().parse(f)

    def get_declared_dependencies(self, session, fixers=None):
        def parse_list(t):
            return [s.strip() for s in t.split(",") if s.strip()]

        description = self._read_description()
        if "Suggests" in description:
            for s in parse_list(description["Suggests"]):
                yield "build", RPackageRequirement.from_str(s)
        if "Depends" in description:
            for s in parse_list(description["Depends"]):
                yield "build", RPackageRequirement.from_str(s)
        if "Imports" in description:
            for s in parse_list(description["Imports"]):
                yield "build", RPackageRequirement.from_str(s)
        if "LinkingTo" in description:
            for s in parse_list(description["LinkingTo"]):
                yield "build", RPackageRequirement.from_str(s)
        # TODO(jelmer): Suggests

    def get_declared_outputs(self, session, fixers=None):
        description = self._read_description()
        if "Package" in description:
            yield RPackageOutput(description["Package"])


class Meson(BuildSystem):

    name = "meson"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def _setup(self, session):
        if not session.exists("build"):
            session.mkdir("build")
        run_detecting_problems(session, ["meson", "setup", "build"])

    def clean(self, session, resolver):
        self._setup(session)
        run_detecting_problems(session, ["ninja", "-C", "build", "clean"])

    def build(self, session, resolver):
        self._setup(session)
        run_detecting_problems(session, ["ninja", "-C", "build"])

    def dist(self, session, resolver, target_directory, quiet=False):
        self._setup(session)
        with DistCatcher([session.external_path("build/meson-dist")]) as dc:
            try:
                run_detecting_problems(
                    session, ["ninja", "-C", "build", "dist"])
            except UnidentifiedError as e:
                if ("ninja: error: unknown target 'dist', did you mean 'dino'?"
                        in e.lines):
                    raise NotImplementedError from e
                raise
        return dc.copy_single(target_directory)

    def test(self, session, resolver):
        self._setup(session)
        run_detecting_problems(session, ["ninja", "-C", "build", "test"])

    def install(self, session, resolver, install_target):
        self._setup(session)
        run_detecting_problems(session, ["ninja", "-C", "build", "install"])

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "meson.build")):
            logging.debug("Found meson.build, assuming meson package.")
            return Meson(os.path.join(path, "meson.build"))

    def _introspect(self, session, fixers, args):
        ret = run_with_build_fixers(
            fixers,
            session,
            ["meson", "introspect"] + args + ['./meson.build'])
        import json
        return json.loads(''.join(ret))

    def get_declared_dependencies(self, session, fixers=None):
        resp = self._introspect(session, fixers, ["--scan-dependencies"])
        for entry in resp:
            version = entry.get('version', [])
            minimum_version = None
            if len(version) == 1 and version[0].startswith('>='):
                minimum_version = version[0][2:]
            elif len(version) > 1:
                logging.warning(
                    'Unable to parse version constraints: %r', version)
            # TODO(jelmer): Include entry['required']
            yield (
                "core",
                VagueDependencyRequirement(
                    entry['name'], minimum_version=minimum_version))

    def get_declared_outputs(self, session, fixers=None):
        resp = self._introspect(session, fixers, ["--targets"])
        for entry in resp:
            if not entry['installed']:
                continue
            if entry['type'] == 'executable':
                for name in entry['filename']:
                    yield BinaryOutput(os.path.basename(name))
            # TODO(jelmer): Handle other types


class Npm(BuildSystem):

    name = "npm"

    def __init__(self, path):
        import json

        self.path = path

        with open(path, "r") as f:
            self.package = json.load(f)

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def get_declared_dependencies(self, session, fixers=None):
        for name, _version in self.package.get(
                "dependencies", {}).items():
            # TODO(jelmer): Look at version
            yield "core", NodePackageRequirement(name)
        for name, _version in self.package.get(
                "devDependencies", {}).items():
            # TODO(jelmer): Look at version
            yield "build", NodePackageRequirement(name)

    def setup(self, session, resolver):
        binary_req = BinaryRequirement("npm")
        if not binary_req.met(session):
            resolver.install([binary_req])

    def dist(self, session, resolver, target_directory, quiet=False):
        self.setup(session, resolver)
        with DistCatcher([session.external_path(".")]) as dc:
            run_detecting_problems(session, ["npm", "pack"])
        return dc.copy_single(target_directory)

    def test(self, session, resolver):
        self.setup(session, resolver)
        test_script = self.package.get("scripts", {}).get("test")
        if test_script:
            run_detecting_problems(session, ['bash', '-c', test_script])
        else:
            logging.info('No test command defined in package.json')

    def build(self, session, resolver):
        self.setup(session, resolver)
        build_script = self.package.get("scripts", {}).get("build")
        if build_script:
            run_detecting_problems(session, ['bash', '-c', build_script])
        else:
            logging.info('No build command defined in package.json')

    def clean(self, session, resolver):
        self.setup(session, resolver)
        clean_script = self.package.get("scripts", {}).get("clean")
        if clean_script:
            run_detecting_problems(session, ['bash', '-c', clean_script])
        else:
            logging.info('No clean command defined in package.json')

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "package.json")):
            logging.debug("Found package.json, assuming node package.")
            return cls(os.path.join(path, "package.json"))


class Waf(BuildSystem):

    name = "waf"

    def __init__(self, path):
        self.path = path

    def setup(self, session, resolver):
        binary_req = BinaryRequirement("python3")
        if not binary_req.met(session):
            resolver.install([binary_req])

    def build(self, session, resolver):
        try:
            run_detecting_problems(session, [self.path, 'build'])
        except UnidentifiedError as e:
            if ("The project was not configured: run \"waf configure\" first!"
                    in e.lines):
                run_detecting_problems(session, [self.path, 'configure'])
                run_detecting_problems(session, [self.path, 'build'])
            else:
                raise

    def dist(self, session, resolver, target_directory, quiet=False):
        self.setup(session, resolver)
        with DistCatcher.default(session.external_path(".")) as dc:
            run_detecting_problems(session, ["./waf", "dist"])
        return dc.copy_single(target_directory)

    def test(self, session, resolver):
        self.setup(session, resolver)
        run_detecting_problems(session, ["./waf", "test"])

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "waf")):
            logging.debug("Found waf, assuming waf package.")
            return cls(os.path.join(path, "waf"))


class Gem(BuildSystem):

    name = "gem"

    def __init__(self, path):
        self.path = path

    def dist(self, session, resolver, target_directory, quiet=False):
        gemfiles = [
            entry.name for entry in session.scandir(".")
            if entry.name.endswith(".gem")
        ]
        if len(gemfiles) > 1:
            logging.warning("More than one gemfile. Trying the first?")
        with DistCatcher.default(session.external_path(".")) as dc:
            run_detecting_problems(
                session,
                [guaranteed_which(session, resolver, "gem2tgz"),
                 gemfiles[0]])
        return dc.copy_single(target_directory)

    @classmethod
    def probe(cls, path):
        gemfiles = [
            entry.path for entry in os.scandir(path)
            if entry.name.endswith(".gem")
        ]
        if gemfiles:
            return cls(gemfiles[0])


class DistZilla(BuildSystem):

    name = "dist-zilla"

    def __init__(self, path):
        self.path = path
        self.dist_inkt_class = None
        with open(self.path, "rb") as f:
            for line in f:
                if not line.startswith(b";;"):
                    continue
                try:
                    (key, value) = line[2:].strip().split(b"=", 1)
                except ValueError:
                    continue
                if (key.strip() == b"class"
                        and value.strip().startswith(b"'Dist::Inkt")):
                    logging.debug(
                        "Found Dist::Inkt section in dist.ini, "
                        "assuming distinkt."
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

    def dist(self, session, resolver, target_directory, quiet=False):
        self.setup(resolver)
        if self.name == "dist-inkt":
            with DistCatcher.default(session.external_path(".")) as dc:
                run_detecting_problems(
                    session,
                    [guaranteed_which(session, resolver, "distinkt-dist")])
            return dc.copy_single(target_directory)
        else:
            # Default to invoking Dist::Zilla
            with DistCatcher.default(session.external_path(".")) as dc:
                run_detecting_problems(
                    session,
                    [guaranteed_which(session, resolver, "dzil"),
                     "build", "--tgz"])
            return dc.copy_single(target_directory)

    def test(self, session, resolver):
        # see also
        # https://perlmaven.com/how-to-run-the-tests-of-a-typical-perl-module
        self.setup(resolver)
        run_detecting_problems(
            session,
            [guaranteed_which(session, resolver, "dzil"), "test"])

    def build(self, session, resolver):
        self.setup(resolver)
        run_detecting_problems(
            session,
            [guaranteed_which(session, resolver, "dzil"), "build"])

    @classmethod
    def probe(cls, path):
        if (os.path.exists(os.path.join(path, "dist.ini"))
                and not os.path.exists(os.path.join(path, "Makefile.PL"))):
            return cls(os.path.join(path, "dist.ini"))

    def get_declared_dependencies(self, session, fixers=None):
        if os.path.exists(os.path.join(self.path, "dist.ini")):
            lines = run_with_build_fixers(
                fixers, session, ["dzil", "authordeps"])
            for entry in lines:
                yield "build", PerlModuleRequirement(entry.strip())
        if os.path.exists(
                os.path.join(os.path.dirname(self.path), "cpanfile")):
            yield from _declared_deps_from_cpanfile(session, fixers)


class RunTests(BuildSystem):

    name = "runtests"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "runtests.sh")):
            return cls(path)

    def test(self, session, resolver):
        if shebang_binary(os.path.join(self.path, "runtests.sh")) is not None:
            run_detecting_problems(session, ["./runtests.sh"])
        else:
            run_detecting_problems(session, ["/bin/bash", "./runtests.sh"])


def _read_cpanfile(session, args, kind, fixers):
    for line in run_with_build_fixers(
            fixers, session, ["cpanfile-dump"] + args):
        line = line.strip()
        if line:
            yield kind, PerlModuleRequirement(line)


def _declared_deps_from_cpanfile(session, fixers):
    yield from _read_cpanfile(
        session, ["--configure", "--build"], "build", fixers)
    yield from _read_cpanfile(session, ["--test"], "test", fixers)


def _declared_deps_from_meta_yml(f):
    # See http://module-build.sourceforge.net/META-spec-v1.4.html for
    # the specification of the format.
    import ruamel.yaml
    import ruamel.yaml.reader

    try:
        data = ruamel.yaml.load(f, ruamel.yaml.SafeLoader)
    except ruamel.yaml.reader.ReaderError as e:
        warnings.warn("Unable to parse META.yml: %s" % e)
        return
    for require in data.get("requires", None) or []:
        yield "core", PerlModuleRequirement(require)
    for require in data.get("build_requires", None) or []:
        yield "build", PerlModuleRequirement(require)
    for require in data.get("configure_requires", None) or []:
        yield "build", PerlModuleRequirement(require)
    # TODO(jelmer): recommends


class CMake(BuildSystem):

    name = "cmake"

    def __init__(self, path):
        self.path = path
        self.builddir = 'build'

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def setup(self, session, resolver):
        if not session.exists(self.builddir):
            session.mkdir(self.builddir)
        try:
            run_detecting_problems(
                session, ["cmake", '.', '-B%s' % self.builddir])
        except Exception:
            session.rmtree(self.builddir)
            raise

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, 'CMakeLists.txt')):
            return cls(path)
        return None

    def build(self, session, resolver):
        self.setup(session, resolver)
        run_detecting_problems(
            session, ["cmake", "--build", self.builddir])

    def install(self, session, resolver, install_target):
        self.setup(session, resolver)
        run_detecting_problems(
            session, ["cmake", "--install", self.builddir])

    def clean(self, session, resolver):
        self.setup(session, resolver)
        run_detecting_problems(
            session,
            ["cmake", "--build %s" % self.builddir, ".", "--target", "clean"])

    def test(self, session, resolver):
        raise NotImplementedError(self.test)

    def get_declared_dependencies(self, session, fixers=None):
        # TODO(jelmer): Find a proper parser for CMakeLists.txt somewhere?
        with open(os.path.join(self.path, 'CMakeLists.txt'), 'r') as f:
            for line in f:
                m = re.match(r'cmake_minimum_required\(\s*VERSION\s+(.*)\s*\)',
                             line)
                if m:
                    yield "build", VagueDependencyRequirement(
                        'CMake', minimum_version=m.group(1))


class Make(BuildSystem):

    def __init__(self, path):
        self.path = path
        if os.path.exists(os.path.join(path, 'Makefile.PL')):
            self.name = 'makefile.pl'
        elif os.path.exists(os.path.join(path, 'Makefile.am')):
            self.name = 'automake'
        elif any(os.path.exists(os.path.join(path, n))
                 for n in ['configure.ac', 'configure.in', 'autogen.sh']):
            self.name = 'autoconf'
        elif any(n.name.endswith(".pro") for n in os.scandir(path)):
            self.name = 'qmake'
        else:
            self.name = "make"

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def setup(self, session, resolver, prefix=None):
        def makefile_exists():
            return any(
                session.exists(p)
                for p in ["Makefile", "GNUmakefile", "makefile"]
            )

        if session.exists("Makefile.PL") and not makefile_exists():
            run_detecting_problems(session, ["perl", "Makefile.PL"])

        if not makefile_exists() and not session.exists("configure"):
            if session.exists("autogen.sh"):
                if shebang_binary(
                        os.path.join(self.path, "autogen.sh")) is None:
                    run_detecting_problems(
                        session, ["/bin/sh", "./autogen.sh"])
                try:
                    run_detecting_problems(session, ["./autogen.sh"])
                except UnidentifiedError as e:
                    if (
                        "Gnulib not yet bootstrapped; "
                        "run ./bootstrap instead." in e.lines
                    ):
                        run_detecting_problems(session, ["./bootstrap"])
                        run_detecting_problems(session, ["./autogen.sh"])
                    else:
                        raise

            elif (session.exists("configure.ac")
                    or session.exists("configure.in")):
                run_detecting_problems(session, ["autoreconf", "-i"])

        if not makefile_exists() and session.exists("configure"):
            extra_args = []
            if prefix is not None:
                extra_args.append('--prefix=%s' % prefix)
            run_detecting_problems(session, ["./configure"] + extra_args)

        if not makefile_exists() and any(
            n.name.endswith(".pro") for n in session.scandir(".")
        ):
            run_detecting_problems(session, ["qmake"])

    def build(self, session, resolver):
        self.setup(session, resolver)
        if self.name == 'qmake':
            default_target = None
        else:
            default_target = 'all'
        self._run_make(
            session,
            [default_target] if default_target else [])

    def clean(self, session, resolver):
        self.setup(session, resolver)
        self._run_make(session, ["clean"])

    def _run_make(self, session, args, prefix=None):
        def _wants_configure(line):
            if line.startswith("Run ./configure"):
                return True
            if line == "Please run ./configure first":
                return True
            if line.startswith("Project not configured"):
                return True
            if line.startswith("The project was not configured"):
                return True
            return re.match(
                    r'Makefile:[0-9]+: \*\*\* '
                    r'You need to run \.\/configure .*', line)
        if session.exists('build/Makefile'):
            cwd = 'build'
        else:
            cwd = None
        try:
            run_detecting_problems(session, ["make"] + args, cwd=cwd)
        except UnidentifiedError as e:
            if len(e.lines) < 5 and any(
                    _wants_configure(line) for line in e.lines):
                extra_args = []
                if prefix is not None:
                    extra_args.append("--prefix=%s" % prefix)
                run_detecting_problems(session, ["./configure"] + extra_args)
                run_detecting_problems(session, ["make"] + args)
            elif (
                "Reconfigure the source tree "
                "(via './config' or 'perl Configure'), please."
            ) in e.lines:
                run_detecting_problems(session, ["./config"])
                run_detecting_problems(session, ["make"] + args)
            else:
                raise

    def test(self, session, resolver):
        self.setup(session, resolver)
        for target in ["check", "test"]:
            try:
                self._run_make(session, [target])
            except UnidentifiedError as e:
                if (("make: *** No rule to make target '%s'.  Stop." % target)
                        in e.lines):
                    pass
                else:
                    raise
            else:
                break
        else:
            if os.path.isdir('t'):
                # See
                # https://perlmaven.com/how-to-run-the-tests-of-a-typical-perl-module
                run_detecting_problems(session, ["prove", "-b", "t/"])
            else:
                logging.warning('No test target found')

    def install(self, session, resolver, install_target):
        self.setup(session, resolver, prefix=install_target.prefix)
        self._run_make(session, ["install"], prefix=install_target.prefix)

    def dist(self, session, resolver, target_directory, quiet=False):
        self.setup(session, resolver)
        with DistCatcher.default(session.external_path(".")) as dc:
            try:
                self._run_make(session, ["dist"])
            except UnidentifiedError as e:
                if ("make: *** No rule to make target 'dist'.  "  # noqa:SIM114
                        "Stop." in e.lines):
                    raise NotImplementedError from e
                elif ("make[1]: "  # noqa:SIM114
                      "*** No rule to make target 'dist'.  "
                      "Stop." in e.lines):
                    raise NotImplementedError from e
                elif ("ninja: error: unknown target 'dist', "  # noqa: SIM114
                      "did you mean 'dino'?" in e.lines):
                    raise NotImplementedError from e
                elif (
                    "Please try running 'make manifest' and then run "
                    "'make dist' again." in e.lines
                ):
                    run_detecting_problems(session, ["make", "manifest"])
                    run_detecting_problems(session, ["make", "dist"])
                elif any(
                        re.match(
                            r"(Makefile|GNUmakefile|makefile):[0-9]+: "
                            r"\*\*\* Missing \'Make.inc\' "
                            r"Run \'./configure \[options\]\' and retry.  "
                            r"Stop.", line,
                        )
                        for line in e.lines
                ):
                    run_detecting_problems(session, ["./configure"])
                    run_detecting_problems(session, ["make", "dist"])
                elif any(
                        re.match(
                            r"Problem opening MANIFEST: "
                            r"No such file or directory "
                            r"at .* line [0-9]+\.", line,
                        )
                        for line in e.lines
                ):
                    run_detecting_problems(session, ["make", "manifest"])
                    run_detecting_problems(session, ["make", "dist"])
                else:
                    raise
        return dc.copy_single(target_directory)

    def get_declared_dependencies(self, session, fixers=None):
        # TODO(jelmer): Split out the perl-specific stuff?
        if os.path.exists(os.path.join(self.path, "META.yml")):
            with open(os.path.join(self.path, "META.yml"), "rb") as f:
                yield from _declared_deps_from_meta_yml(f)
        if os.path.exists(os.path.join(self.path, "cpanfile")):
            yield from _declared_deps_from_cpanfile(session, fixers)

    @classmethod
    def probe(cls, path):
        if any(
                os.path.exists(os.path.join(path, p))
                for p in [
                    "Makefile",
                    "GNUmakefile",
                    "makefile",
                    "Makefile.PL",
                    "autogen.sh",
                    "configure.ac",
                    "configure.in",
                ]
        ):
            return cls(path)
        for n in os.scandir(path):
            # qmake
            if n.name.endswith(".pro"):
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

    def install_declared_requirements(self, stages, session, resolver, fixers):
        run_with_build_fixers(fixers, session, ["cargo", "fetch"])

    def get_declared_dependencies(self, session, fixers=None):
        if "dependencies" in self.cargo:
            for name, details in self.cargo["dependencies"].items():
                if isinstance(details, str):
                    details = {"version": details}
                # TODO(jelmer): Look at details['version']
                yield "build", CargoCrateRequirement(
                    name,
                    features=details.get("features", []),
                    minimum_version=details.get("version"),
                )

    def test(self, session, resolver):
        run_detecting_problems(session, ["cargo", "test"])

    def clean(self, session, resolver):
        run_detecting_problems(session, ["cargo", "clean"])

    def build(self, session, resolver):
        try:
            run_detecting_problems(session, ["cargo", "generate"])
        except UnidentifiedError as e:
            if e.lines != ['error: no such subcommand: `generate`']:
                raise
        run_detecting_problems(session, ["cargo", "build"])

    def install(self, session, resolver, install_target):
        args = []
        if install_target.prefix:
            args.append('-root=%s' % install_target.prefix)
        run_detecting_problems(
            session, ["cargo", "install", "--path=."] + args)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "Cargo.toml")):
            logging.debug("Found Cargo.toml, assuming rust cargo package.")
            return Cargo(os.path.join(path, "Cargo.toml"))


def _parse_go_mod(f):
    def readline():
        line = f.readline()
        if not line:
            return line
        return line.split("//")[0] + "\n"

    line = readline()
    while line:
        parts = line.strip().split(" ")
        if not parts or parts == [""]:
            line = readline()
            continue
        if len(parts) == 2 and parts[1] == "(":
            line = readline()
            while line.strip() != ")":
                yield [parts[0]] + list(line.strip().split(" "))
                line = readline()
                if not line:
                    raise AssertionError("list of %s interrupted?" % parts[0])
        else:
            yield parts
        line = readline()


class Golang(BuildSystem):
    """Go builds."""

    name = "golang"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s()" % (type(self).__name__)

    def test(self, session, resolver):
        run_detecting_problems(session, ["go", "test", "./..."])

    def build(self, session, resolver):
        run_detecting_problems(session, ["go", "build"])

    def install(self, session, resolver, install_target):
        run_detecting_problems(session, ["go", "install"])

    def clean(self, session, resolver):
        session.check_call(["go", "clean"])

    def get_declared_dependencies(self, session, fixers=None):
        go_mod_path = os.path.join(self.path, "go.mod")
        if os.path.exists(go_mod_path):
            with open(go_mod_path, "r") as f:
                for parts in _parse_go_mod(f):
                    if parts[0] == "go":
                        yield "build", GoRequirement(parts[1])
                    elif parts[0] == "require":
                        yield "build", GoPackageRequirement(
                            parts[1],
                            parts[2].lstrip("v") if len(parts) > 2 else None
                        )
                    elif parts[0] == "exclude":  # noqa: SIM114
                        pass  # TODO(jelmer): Create conflicts?
                    elif parts[0] == "replace":  # noqa: SIM114
                        pass  # TODO(jelmer): do.. something?
                    elif parts[0] == "module":  # noqa: SIM114
                        pass
                    else:
                        logging.warning(
                            "Unknown directive %s in go.mod", parts[0])

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "go.mod")):
            return Golang(path)
        if os.path.exists(os.path.join(path, "go.sum")):
            return Golang(path)
        for entry in os.scandir(path):
            if entry.name.endswith(".go"):
                return Golang(path)
            if entry.is_dir():
                for subentry in os.scandir(entry.path):
                    if subentry.name.endswith(".go"):
                        return Golang(path)


class Maven(BuildSystem):

    name = "maven"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "pom.xml")):
            logging.debug("Found pom.xml, assuming maven package.")
            return cls(os.path.join(path, "pom.xml"))

    def test(self, session, resolver):
        run_detecting_problems(session, ["mvn", "test"])

    def clean(self, session, resolver):
        run_detecting_problems(session, ["mvn", "clean"])

    def install(self, session, resolver, install_target):
        run_detecting_problems(session, ["mvn", "install"])

    def build(self, session, resolver):
        run_detecting_problems(session, ["mvn", "compile"])

    def dist(self, session, resolver, target_directory, quiet=False):
        # TODO(jelmer): 'mvn generate-sources' creates a jar in target/.
        # is that what we need?
        raise NotImplementedError

    def get_declared_dependencies(self, session, fixers=None):
        import xml.etree.ElementTree as ET

        try:
            root = xmlparse_simplify_namespaces(
                self.path, ["http://maven.apache.org/POM/4.0.0"]
            )
        except ET.ParseError as e:
            logging.warning("Unable to parse package.xml: %s", e)
            return
        assert root.tag == "project", "root tag is %r" % root.tag
        deps_tag = root.find("dependencies")
        if deps_tag:
            for dep in deps_tag.findall("dependency"):
                version_tag = dep.find("version")
                yield "core", MavenArtifactRequirement(
                    dep.find("groupId").text,
                    dep.find("artifactId").text,
                    version_tag.text if version_tag else None,
                )


class Cabal(BuildSystem):

    name = "cabal"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def _run(self, session, args):
        try:
            run_detecting_problems(
                session, ["runhaskell", "Setup.hs"] + args)
        except UnidentifiedError as e:
            if "Run the 'configure' command first." in e.lines:
                run_detecting_problems(
                    session, ["runhaskell", "Setup.hs", "configure"])
                run_detecting_problems(
                    session, ["runhaskell", "Setup.hs"] + args)
            else:
                raise

    def test(self, session, resolver):
        self._run(session, ["test"])

    def dist(self, session, resolver, target_directory, quiet=False):
        with DistCatcher(
            [
                session.external_path("dist-newstyle/sdist"),
                session.external_path("dist"),
            ]
        ) as dc:
            self._run(session, ["sdist"])
        return dc.copy_single(target_directory)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "Setup.hs")):
            logging.debug("Found Setup.hs, assuming haskell package.")
            return cls(os.path.join(path, "Setup.hs"))


class Composer(BuildSystem):

    name = "composer"

    def __init__(self, path):
        self.path = path

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "composer.json")):
            logging.debug("Found composer.json, assuming composer package.")
            return cls(path)


class PerlBuildTiny(BuildSystem):

    name = "perl-build-tiny"

    def __init__(self, path):
        self.path = path
        self.minilla = os.path.exists(os.path.join(self.path, "minil.toml"))

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.path)

    def setup(self, session, fixers=None):
        run_with_build_fixers(fixers, session, ["perl", "Build.PL"])

    def test(self, session, resolver):
        self.setup(session)
        if self.minilla:
            run_detecting_problems(session, ["minil", "test"])
        else:
            run_detecting_problems(session, ["./Build", "test"])

    def build(self, session, resolver):
        self.setup(session)
        run_detecting_problems(session, ["./Build", "build"])

    def clean(self, session, resolver):
        self.setup(session)
        run_detecting_problems(session, ["./Build", "clean"])

    def dist(self, session, resolver, target_directory, quiet=False):
        self.setup(session)
        with DistCatcher([session.external_path('.')]) as dc:
            if self.minilla:
                # minil seems to return 0 even if it didn't produce a tarball
                # :(
                run_detecting_problems(
                    session, ["minil", "dist"],
                    check_success=lambda retcode, lines: bool(dc.find_files()))
            else:
                try:
                    run_detecting_problems(session, ["./Build", "dist"])
                except UnidentifiedError as e:
                    if ("Can't find dist packages without a MANIFEST file"
                            in e.lines):
                        run_detecting_problems(
                            session, ["./Build", "manifest"])
                        run_detecting_problems(session, ["./Build", "dist"])
                    elif "No such action 'dist'" in e.lines:
                        raise NotImplementedError from e
                    else:
                        raise
        return dc.copy_single(target_directory)

    def install(self, session, resolver, install_target):
        self.setup(session)
        if self.minilla:
            run_detecting_problems(session, ["minil", "install"])
        else:
            run_detecting_problems(session, ["./Build", "install"])

    def get_declared_dependencies(self, session, fixers=None):
        self.setup(session, fixers)
        if self.minilla:
            # Minilla doesn't seem to have a way to just regenerate the
            # metadata :(
            pass
        else:
            try:
                run_with_build_fixers(fixers, session, ["./Build", "distmeta"])
            except UnidentifiedError as e:
                if "No such action 'distmeta'" in e.lines:
                    pass
                if ("Do not run distmeta. "
                        "Install Minilla and `minil install` instead."
                        in e.lines):
                    self.minilla = True
                else:
                    raise
        with suppress(FileNotFoundError), \
                open(os.path.join(self.path, 'META.yml'), 'r') as f:
            yield from _declared_deps_from_meta_yml(f)

    @classmethod
    def probe(cls, path):
        if os.path.exists(os.path.join(path, "Build.PL")):
            logging.debug(
                "Found Build.PL, assuming Module::Build::Tiny package.")
            return cls(path)


BUILDSYSTEM_CLSES: List[Type[BuildSystem]] = [
    Pear,
    SetupPy,
    Npm,
    Waf,
    Meson,
    Cargo,
    Cabal,
    Gradle,
    Maven,
    DistZilla,
    Gem,
    PerlBuildTiny,
    Golang,
    R,
    Octave,
    Bazel,
    CMake,
    GnomeShellExtension,
    # Make is intentionally at the end of the list.
    Make,
    Composer,
    RunTests,
]


def lookup_buildsystem_cls(name: str) -> Type[BuildSystem]:
    for bs in BUILDSYSTEM_CLSES:
        if bs.name == name:
            return bs
    raise KeyError(name)


def scan_buildsystems(path: str) -> List[Tuple[str, BuildSystem]]:
    """Detect build systems."""
    ret = []
    ret.extend([(".", bs) for bs in detect_buildsystems(path) if bs])

    if not ret:
        # Nothing found. Try the next level?
        for entry in os.scandir(path):
            if entry.is_dir():
                ret.extend([
                    (entry.name, bs)
                    for bs in detect_buildsystems(entry.path)])

    return ret


def detect_buildsystems(path: str) -> Iterable[BuildSystem]:
    for bs_cls in BUILDSYSTEM_CLSES:
        bs = bs_cls.probe(path)
        if bs is not None:
            yield bs


def get_buildsystem(path: str) -> Tuple[str, BuildSystem]:
    for subpath, buildsystem in scan_buildsystems(path):
        return subpath, buildsystem

    raise NoBuildToolsFound()

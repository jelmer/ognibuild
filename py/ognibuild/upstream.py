#!/usr/bin/python3
# Copyright (C) 2020-2021 Jelmer Vernooij <jelmer@jelmer.uk>
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
import re
import urllib.error
from dataclasses import dataclass, field
from typing import Any, Optional
from urllib.request import Request, urlopen

from debian.changelog import Version

from . import USER_AGENT, Requirement
from .requirements import (
    CargoCrateRequirement,
    GoPackageRequirement,
    PythonPackageRequirement,
    RubyGemRequirement,
)
from .resolver.apt import AptRequirement, OneOfRequirement


@dataclass
class UpstreamInfo:
    name: Optional[str]
    buildsystem: Optional[str] = None
    branch_url: Optional[str] = None
    branch_subpath: Optional[str] = None
    tarball_url: Optional[str] = None
    metadata: dict[str, Any] = field(default_factory=dict)

    @property
    def version(self):
        return self.metadata.get("Version")

    def json(self):
        return {
            "name": self.name,
            "buildsystem": self.buildsystem,
            "branch_url": self.branch_url,
            "branch_subpath": self.branch_subpath,
            "tarball_url": self.tarball_url,
            "version": self.version,
        }


def go_base_name(package):
    (hostname, path) = package.split("/", 1)
    if hostname == "github.com":
        hostname = "github"
    if hostname == "gopkg.in":
        hostname = "gopkg"
    path = path.rstrip("/").replace("/", "-")
    if path.endswith(".git"):
        path = path[:-4]
    return (hostname + path).replace("_", "-").lower()


def load_crate_info(crate):
    import json

    http_url = f"https://crates.io/api/v1/crates/{crate}"
    headers = {"User-Agent": USER_AGENT, "Accept": "application/json"}
    try:
        resp = urlopen(Request(http_url, headers=headers))
    except urllib.error.HTTPError as e:
        if e.code == 404:
            logging.warning("No crate %r", crate)
            return None
        raise
    return json.loads(resp.read())


def find_python_package_upstream(requirement):
    return pypi_upstream_info(requirement.package)


def pypi_upstream_info(project, version=None):
    import json

    http_url = f"https://pypi.org/pypi/{project}/json"
    headers = {"User-Agent": USER_AGENT, "Accept": "application/json"}
    try:
        http_contents = urlopen(Request(http_url, headers=headers)).read()
    except urllib.error.HTTPError as e:
        if e.code == 404:
            logging.warning("No pypi project %r", project)
            return None
        raise
    pypi_data = json.loads(http_contents)
    upstream_branch = None
    project_urls = pypi_data["info"]["project_urls"]
    for name, url in (project_urls or {}).items():
        if name.lower() in ("github", "repository"):
            upstream_branch = url
    tarball_url = None
    for url_data in pypi_data["urls"]:
        if url_data.get("package_type") == "sdist":
            tarball_url = url_data["url"]
    return UpstreamInfo(
        branch_url=upstream_branch,
        branch_subpath="",
        name="python-{}".format(pypi_data["info"]["name"]),
        tarball_url=tarball_url,
    )


def find_go_package_upstream(requirement):
    if requirement.package.startswith("github.com/"):
        metadata = {
            "Go-Import-Path": requirement.package,
        }
        return UpstreamInfo(
            name=f"golang-{go_base_name(requirement.package)}",
            metadata=metadata,
            branch_url="https://{}".format("/".join(requirement.package.split("/")[:3])),
            branch_subpath="",
        )


def find_perl_module_upstream(requirement):
    return perl_upstream_info(requirement.module)


def cargo_upstream_info(crate, version=None, api_version=None):
    import semver
    from debmutate.debcargo import semver_pair

    data = load_crate_info(crate)
    if data is None:
        return None
    # TODO(jelmer): Use upstream ontologist to parse upstream metadata
    upstream_branch = data["crate"]["repository"]
    name = "rust-" + data["crate"]["name"].replace("_", "-")
    version = None
    if version is not None:
        pass
    elif api_version is not None:
        for version_info in data["versions"]:
            if (
                not version_info["num"].startswith(api_version + ".")
                and version_info["num"] != api_version
            ):
                continue
            if version is None:
                version = semver.VersionInfo.parse(version_info["num"])
            else:
                version = semver.max_ver(
                    version, semver.VersionInfo.parse(version_info["num"])
                )
        if version is None:
            logging.warning(
                "Unable to find version of crate %s "
                "that matches API version %s",
                name,
                api_version,
            )
        else:
            name += "-" + semver_pair(str(version))
    metadata = {"Cargo-Crate": data["crate"]["name"]}
    if version:
        metadata["Version"] = str(version)

    return UpstreamInfo(
        branch_url=upstream_branch,
        branch_subpath=None,
        name=name,
        metadata=metadata,
        buildsystem="cargo",
    )


def find_cargo_crate_upstream(requirement):
    return cargo_upstream_info(
        requirement.crate, api_version=requirement.api_version
    )


def apt_to_cargo_requirement(m, rels):
    name = m.group(1)
    api_version = m.group(2)
    if m.group(3):
        features = set(m.group(3)[1:].split("-"))
    else:
        features = set()
    if not rels:
        minimum_version = None
    elif len(rels) == 1 and rels[0][0] == ">=":
        minimum_version = Version(rels[0][1]).upstream_version
    else:
        logging.warning("Unable to parse Debian version %r", rels)
        minimum_version = None

    return CargoCrateRequirement(
        name,
        api_version=api_version,
        features=features,
        minimum_version=minimum_version,
    )


def apt_to_python_requirement(m, rels):
    name = m.group(2)
    python_version = m.group(1)
    if not rels:
        minimum_version = None
    elif len(rels) == 1 and rels[0][0] == ">=":
        minimum_version = Version(rels[0][1]).upstream_version
    else:
        logging.warning("Unable to parse Debian version %r", rels)
        minimum_version = None
    return PythonPackageRequirement(
        name,
        python_version=(python_version or None),
        minimum_version=minimum_version,
    )


def apt_to_ruby_requirement(m, rels):
    if not rels:
        minimum_version = None
    elif len(rels) == 1 and rels[0][0] == ">=":
        minimum_version = Version(rels[0][1]).upstream_version
    else:
        logging.warning("Unable to parse Debian version %r", rels)
        minimum_version = None
    return RubyGemRequirement(m.group(1), minimum_version)


def apt_to_go_requirement(m, rels):
    parts = m.group(1).split("-")
    if parts[0] == "github":
        parts[0] = "github.com"
    if parts[0] == "gopkg":
        parts[0] = "gopkg.in"
    if not rels:
        version = None
    elif len(rels) == 1 and rels[0][0] == "=":
        version = Version(rels[0][1]).upstream_version
    else:
        logging.warning("Unable to parse Debian version %r", rels)
        version = None
    return GoPackageRequirement("/".join(parts), version=version)


BINARY_PACKAGE_UPSTREAM_MATCHERS = [
    (r"librust-(.*)-([^-+]+)(\+.*?)-dev", apt_to_cargo_requirement),
    (r"python([0-9.]*)-(.*)", apt_to_python_requirement),
    (r"golang-(.*)-dev", apt_to_go_requirement),
    (r"ruby-(.*)", apt_to_ruby_requirement),
]


_BINARY_PACKAGE_UPSTREAM_MATCHERS = [
    (re.compile(r), fn) for (r, fn) in BINARY_PACKAGE_UPSTREAM_MATCHERS
]


def find_apt_upstream(requirement: AptRequirement) -> Optional[UpstreamInfo]:
    for option in requirement.relations:
        for rel in option:
            for matcher, fn in _BINARY_PACKAGE_UPSTREAM_MATCHERS:
                m = matcher.fullmatch(rel["name"])
                if m:
                    upstream_requirement = fn(
                        m, [rel["version"]] if rel["version"] else []
                    )
                    return find_upstream(upstream_requirement)

            logging.warning(
                "Unable to map binary package name %s to upstream", rel["name"]
            )
    return None


def find_or_upstream(requirement: OneOfRequirement) -> Optional[UpstreamInfo]:
    for req in requirement.elements:
        info = find_upstream(req)
        if info is not None:
            return info
    return None


def load_npm_package(package):
    import json

    http_url = f"https://registry.npmjs.org/{package}"
    headers = {"User-Agent": USER_AGENT, "Accept": "application/json"}
    try:
        resp = urlopen(Request(http_url, headers=headers))
    except urllib.error.HTTPError as e:
        if e.code == 404:
            logging.warning("No npm package %r", package)
            return None
        raise
    return json.loads(resp.read())


def npm_upstream_info(package, version=None):
    data = load_npm_package(package)
    if data is None:
        return None
    versions = data.get("versions", {})
    if version is not None:
        version_data = versions[version]
    else:
        version_data = versions[max(versions.keys())]
    if "repository" in version_data:
        try:
            branch_url = version_data["repository"]["url"]
        except (TypeError, KeyError):
            logging.warning(
                "Unexpectedly formatted repository data: %r",
                version_data["repository"],
            )
            branch_url = None
    else:
        branch_url = None
    return UpstreamInfo(
        branch_url=branch_url,
        branch_subpath="",
        name=f"node-{package}",
        tarball_url=version_data["dist"]["tarball"],
    )


def find_npm_upstream(requirement):
    return npm_upstream_info(requirement.package)


def load_cpan_module(module):
    import json

    http_url = f"https://fastapi.metacpan.org/v1/module/{module}?join=release"
    headers = {"User-Agent": USER_AGENT, "Accept": "application/json"}
    try:
        resp = urlopen(Request(http_url, headers=headers))
    except urllib.error.HTTPError as e:
        if e.code == 404:
            logging.warning("No CPAN module %r", module)
            return None
        raise
    return json.loads(resp.read())


def perl_upstream_info(module, version=None):
    data = load_cpan_module(module)
    if data is None:
        return None
    release_metadata = data["release"]["_source"]["metadata"]
    release_resources = release_metadata.get("resources", {})
    branch_url = release_resources.get("repository", {}).get("url")
    metadata = {}
    metadata["Version"] = data["version"]
    return UpstreamInfo(
        name="lib{}-perl".format(module.lower().replace("::", "-")),
        metadata=metadata,
        branch_url=branch_url,
        branch_subpath="",
        tarball_url=data["download_url"],
    )


def load_hackage_package(package, version=None):
    headers = {"User-Agent": USER_AGENT}
    http_url = f"https://hackage.haskell.org/package/{package}/{package}.cabal"
    try:
        resp = urlopen(Request(http_url, headers=headers))
    except urllib.error.HTTPError as e:
        if e.code == 404:
            logging.warning("No hackage package %r", package)
            return None
        raise
    return resp.read()


def haskell_upstream_info(package, version=None):
    data = load_hackage_package(package, version)
    if data is None:
        return None
    # TODO(jelmer): parse cabal file
    # upstream-ontologist has a parser..
    return UpstreamInfo(name=f"haskell-{package}")


def find_haskell_package_upstream(requirement):
    return haskell_upstream_info(requirement.package)


def load_rubygem(gem):
    headers = {"User-Agent": USER_AGENT}
    http_url = f"https://rubygems.org/api/v1/gems/{gem}.json"
    try:
        resp = urlopen(Request(http_url, headers=headers))
    except urllib.error.HTTPError as e:
        if e.code == 404:
            logging.warning("No rubygem %r", gem)
            return None
        raise
    return json.load(resp)


def rubygem_upstream_info(gem):
    data = load_rubygem(gem)
    if data is None:
        return None
    metadata = {}
    homepage = data.get("homepage_uri")
    if homepage:
        metadata["Homepage"] = homepage
    bug_tracker = data.get("bug_tracker_uri")
    if bug_tracker:
        metadata["Bug-Database"] = bug_tracker
    metadata["Version"] = data["version"]
    return UpstreamInfo(
        name=f"ruby-{gem}",
        branch_url=data["source_code_uri"],
        metadata=metadata,
    )


def find_rubygem_upstream(req):
    return rubygem_upstream_info(req.gem)


UPSTREAM_FINDER = {
    "python-package": find_python_package_upstream,
    "npm-package": find_npm_upstream,
    "go-package": find_go_package_upstream,
    "perl-module": find_perl_module_upstream,
    "cargo-crate": find_cargo_crate_upstream,
    "haskell-package": find_haskell_package_upstream,
    "apt": find_apt_upstream,
    "or": find_or_upstream,
    "gem": find_rubygem_upstream,
}


def find_upstream(requirement: Requirement) -> Optional[UpstreamInfo]:
    try:
        return UPSTREAM_FINDER[requirement.family](requirement)
    except KeyError:
        return None


def find_upstream_from_repology(name, version=None) -> Optional[UpstreamInfo]:
    if ":" not in name:
        return None
    family, name = name.split(":")
    if family == "python":
        return pypi_upstream_info(name, version)
    if family == "go":
        parts = name.split("-")
        if parts[0] == "github":
            parts[0] = "github.com"
        return UpstreamInfo(
            name=f"golang-{name}",
            branch_url="https://" + "/".join(parts),
            branch_subpath="",
        )
    if family == "rust":
        return cargo_upstream_info(name, version=version)
    if family == "node":
        return npm_upstream_info(name, version)
    if family == "perl":
        module = "::".join([x.capitalize() for x in name.split("-")])
        return perl_upstream_info(module, version)
    if family == "haskell":
        return haskell_upstream_info(name, version)
    # apmod, coq, cursors, deadbeef, emacs, erlang, fonts, fortunes, fusefs,
    # gimp, gstreamer, gtktheme, haskell, raku, ros, haxe, icons, java, js,
    # julia, ladspa, lisp, lua, lv2, mingw, nextcloud, nginx, nim, ocaml,
    # opencpn, rhythmbox texlive, tryton, vapoursynth, vdr, vim, xdrv,
    # xemacs
    return None


if __name__ == "__main__":
    import argparse
    import sys

    parser = argparse.ArgumentParser()
    parser.add_argument("--json", action="store_true")
    parser.add_argument("name", type=str)
    parser.add_argument("version", type=str, nargs="?", default=None)
    args = parser.parse_args()

    logging.basicConfig(format="%(message)s", level=logging.INFO)

    upstream_info = find_upstream_from_repology(args.name, args.version)
    if upstream_info is None:
        logging.fatal(
            "Unable to find upstream info for repology %s", args.name
        )
        sys.exit(1)

    if args.json:
        import json

        json.dump(upstream_info.json(), sys.stdout)
        sys.exit(0)

    if upstream_info.name:
        logging.info("Name: %s", upstream_info.name)
    if upstream_info.version:
        logging.info("Version: %s", upstream_info.version)
    if upstream_info.buildsystem:
        logging.info("Buildsystem: %s", upstream_info.buildsystem)
    if upstream_info.branch_url:
        logging.info(
            "Branch: %s [%s]",
            upstream_info.branch_url,
            upstream_info.branch_subpath,
        )
    if upstream_info.tarball_url:
        logging.info("Tarball URL: %s", upstream_info.tarball_url)

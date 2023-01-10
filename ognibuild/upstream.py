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

from dataclasses import dataclass, field
from typing import Optional, Dict, Any
from debian.changelog import Version
import logging
import re

from . import Requirement, USER_AGENT
from .requirements import (
    CargoCrateRequirement,
    GoPackageRequirement,
    PythonPackageRequirement,
)
from .resolver.apt import AptRequirement, OneOfRequirement


@dataclass
class UpstreamInfo:
    name: Optional[str]
    buildsystem: Optional[str] = None
    branch_url: Optional[str] = None
    branch_subpath: Optional[str] = None
    tarball_url: Optional[str] = None
    version: Optional[str] = None
    metadata: Dict[str, Any] = field(default_factory=dict)

    def json(self):
        return {
            'name': self.name,
            'buildsystem': self.buildsystem,
            'branch_url': self.branch_url,
            'branch_subpath': self.branch_subpath,
            'tarball_url': self.tarball_url,
            'version': self.version
        }


def go_base_name(package):
    (hostname, path) = package.split('/', 1)
    if hostname == "github.com":
        hostname = "github"
    if hostname == "gopkg.in":
        hostname = "gopkg"
    path = path.rstrip('/').replace("/", "-")
    if path.endswith('.git'):
        path = path[:-4]
    return (hostname + path).replace("_", "-").lower()


def load_crate_info(crate):
    import urllib.error
    from urllib.request import urlopen, Request
    import json
    http_url = 'https://crates.io/api/v1/crates/%s' % crate
    headers = {'User-Agent': USER_AGENT, 'Accept': 'application/json'}
    http_contents = urlopen(Request(http_url, headers=headers)).read()
    try:
        return json.loads(http_contents)
    except urllib.error.HTTPError as e:
        if e.code == 404:
            logging.warning('No crate %r', crate)
            return None
        raise


def find_python_package_upstream(requirement):
    return pypi_upstream_info(requirement.package)


def pypi_upstream_info(project):
    import urllib.error
    from urllib.request import urlopen, Request
    import json
    http_url = 'https://pypi.org/pypi/%s/json' % project
    headers = {'User-Agent': USER_AGENT, 'Accept': 'application/json'}
    try:
        http_contents = urlopen(
            Request(http_url, headers=headers)).read()
    except urllib.error.HTTPError as e:
        if e.code == 404:
            logging.warning('No pypi project %r', project)
            return None
        raise
    pypi_data = json.loads(http_contents)
    upstream_branch = None
    project_urls = pypi_data['info']['project_urls']
    for name, url in (project_urls or {}).items():
        if name.lower() in ('github', 'repository'):
            upstream_branch = url
    tarball_url = None
    for url_data in pypi_data['urls']:
        if url_data.get('package_type') == 'sdist':
            tarball_url = url_data['url']
    return UpstreamInfo(
        branch_url=upstream_branch, branch_subpath='',
        name='python-%s' % pypi_data['info']['name'],
        tarball_url=tarball_url)


def find_go_package_upstream(requirement):
    if requirement.package.startswith('github.com/'):
        return UpstreamInfo(
            name='golang-%s' % go_base_name(requirement.package),
            branch_url='https://%s' % '/'.join(
                requirement.package.split('/')[:3]),
            branch_subpath='')


def cargo_upstream_info(crate, api_version=None):
    import semver
    from debmutate.debcargo import semver_pair
    data = load_crate_info(crate)
    if data is None:
        return None
    upstream_branch = data['crate']['repository']
    name = 'rust-' + data['crate']['name'].replace('_', '-')
    version = None
    if api_version is not None:
        for version_info in data['versions']:
            if (not version_info['num'].startswith(
                        api_version + '.')
                    and version_info['num'] != api_version):
                continue
            if version is None:
                version = semver.VersionInfo.parse(version_info['num'])
            else:
                version = semver.max_ver(
                    version, semver.VersionInfo.parse(version_info['num']))
        if version is None:
            logging.warning(
                'Unable to find version of crate %s '
                'that matches API version %s',
                name, api_version)
        else:
            name += '-' + semver_pair(str(version))
    return UpstreamInfo(
        branch_url=upstream_branch, branch_subpath=None,
        name=name, version=str(version) if version else None,
        metadata={'X-Cargo-Crate': data['crate']['name']},
        buildsystem='cargo')


def find_cargo_crate_upstream(requirement):
    return cargo_upstream_info(
        requirement.crate, api_version=requirement.api_version)


def apt_to_cargo_requirement(m, rels):
    name = m.group(1)
    api_version = m.group(2)
    if m.group(3):
        features = set(m.group(3)[1:].split('-'))
    else:
        features = set()
    if not rels:
        minimum_version = None
    elif len(rels) == 1 and rels[0][0] == '>=':
        minimum_version = Version(rels[0][1]).upstream_version
    else:
        logging.warning('Unable to parse Debian version %r', rels)
        minimum_version = None

    return CargoCrateRequirement(
        name, api_version=api_version,
        features=features, minimum_version=minimum_version)


def apt_to_python_requirement(m, rels):
    name = m.group(2)
    python_version = m.group(1)
    if not rels:
        minimum_version = None
    elif len(rels) == 1 and rels[0][0] == '>=':
        minimum_version = Version(rels[0][1]).upstream_version
    else:
        logging.warning('Unable to parse Debian version %r', rels)
        minimum_version = None
    return PythonPackageRequirement(
        name, python_version=(python_version or None),
        minimum_version=minimum_version)


def apt_to_go_requirement(m, rels):
    parts = m.group(1).split('-')
    if parts[0] == 'github':
        parts[0] = 'github.com'
    if parts[0] == 'gopkg':
        parts[0] = 'gopkg.in'
    if not rels:
        version = None
    elif len(rels) == 1 and rels[0][0] == '=':
        version = Version(rels[0][1]).upstream_version
    else:
        logging.warning('Unable to parse Debian version %r', rels)
        version = None
    return GoPackageRequirement('/'.join(parts), version=version)


BINARY_PACKAGE_UPSTREAM_MATCHERS = [
    (r'librust-(.*)-([^-+]+)(\+.*?)-dev', apt_to_cargo_requirement),
    (r'python([0-9.]*)-(.*)', apt_to_python_requirement),
    (r'golang-(.*)-dev', apt_to_go_requirement),
]


_BINARY_PACKAGE_UPSTREAM_MATCHERS = [
    (re.compile(r), fn) for (r, fn) in BINARY_PACKAGE_UPSTREAM_MATCHERS]


def find_apt_upstream(requirement: AptRequirement) -> Optional[UpstreamInfo]:
    for option in requirement.relations:
        for rel in option:
            for matcher, fn in _BINARY_PACKAGE_UPSTREAM_MATCHERS:
                m = matcher.fullmatch(rel['name'])
                if m:
                    upstream_requirement = fn(
                        m, [rel['version']] if rel['version'] else [])
                    return find_upstream(upstream_requirement)

            logging.warning(
                'Unable to map binary package name %s to upstream',
                rel['name'])
    return None


def find_or_upstream(requirement: OneOfRequirement) -> Optional[UpstreamInfo]:
    for req in requirement.elements:
        info = find_upstream(req)
        if info is not None:
            return info
    return None


UPSTREAM_FINDER = {
    'python-package': find_python_package_upstream,
    'go-package': find_go_package_upstream,
    'cargo-crate': find_cargo_crate_upstream,
    'apt': find_apt_upstream,
    'or': find_or_upstream,
    }


def find_upstream(requirement: Requirement) -> Optional[UpstreamInfo]:
    try:
        return UPSTREAM_FINDER[requirement.family](requirement)
    except KeyError:
        return None


def find_upstream_from_repology(name) -> Optional[UpstreamInfo]:
    if ':' not in name:
        return None
    family, name = name.split(':')
    if family == 'python':
        return pypi_upstream_info(name)
    if family == 'go':
        parts = name.split('-')
        if parts[0] == 'github':
            parts[0] = 'github.com'
        return UpstreamInfo(
            name=f'golang-{name}',
            branch_url='https://' + '/'.join(parts),
            branch_subpath='')
    if family == 'rust':
        return cargo_upstream_info(name)
    return None

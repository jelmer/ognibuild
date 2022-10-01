#!/usr/bin/python3
# Copyright (C) 2022 Jelmer Vernooij <jelmer@jelmer.uk>
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


import json
from typing import List

from urllib.parse import urljoin
from urllib.request import Request, urlopen


from .. import Requirement, version_string
from .apt import AptRequirement, AptResolver


def resolve_apt_requirement_dep_server(
        url: str, req: Requirement) -> List[Requirement]:
    request = Request(
        urljoin(url, 'resolve-apt'),
        data=json.dumps({'resolve': req.json()}).encode('utf-8'), headers={
            'User-Agent': 'ognibuild/%s' % version_string,
            'Content-Type': 'application/json'})
    resp = urlopen(request)
    ret = json.load(resp)
    return [AptRequirement._from_json(e) for e in ret]


class DepServerAptResolver(AptResolver):
    def __init__(self, apt, dep_server_url, tie_breakers=None):
        super(DepServerAptResolver, self).__init__(
            apt, tie_breakers=tie_breakers)
        self.dep_server_url = dep_server_url

    @classmethod
    def from_session(cls, session, dep_server_url, tie_breakers=None):
        return cls(
            AptManager.from_session(session), dep_server_url,
            tie_breakers=tie_breakers)

    def resolve_all(self, req: Requirement):
        try:
            req.json()
        except NotImplementedError:
            return super(DepServerAptResolver, self).__init__(req)
        return resolve_apt_requirement_dep_server(self.dep_server_url, req)

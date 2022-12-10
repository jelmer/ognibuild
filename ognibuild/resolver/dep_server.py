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

import asyncio
import logging
from typing import List

from aiohttp import (
    ClientSession,
    ClientConnectorError,
    ClientResponseError,
    ServerDisconnectedError,
)
from yarl import URL


from .. import Requirement, USER_AGENT
from ..debian.apt import AptManager
from .apt import AptRequirement, AptResolver


class DepServerError(Exception):

    def __init__(self, inner):
        self.inner = inner


class RequirementFamilyUnknown(DepServerError):

    def __init__(self, family):
        self.family = family


async def resolve_apt_requirement_dep_server(
        url: str, req: Requirement) -> List[AptRequirement]:
    """Resolve a requirement to an APT requirement with a dep server.

    Args:
      url: Dep server URL
      req: Requirement to resolve
    Returns:
      List of Apt requirements.
    """
    async with ClientSession() as session:
        try:
            async with session.post(URL(url) / "resolve-apt", headers={
                    'User-Agent': USER_AGENT},
                    json={'requirement': req.json()},
                    raise_for_status=True) as resp:
                return [
                    AptRequirement._from_json(e) for e in await resp.json()]
        except ClientResponseError as e:
            if e.status == 404:  # noqa: SIM102
                if e.headers.get('Reason') == 'family-unknown':  # type: ignore
                    raise RequirementFamilyUnknown(family=req.family) from e
            raise DepServerError(e) from e
        except (ClientConnectorError,
                ServerDisconnectedError) as e:
            logging.warning('Unable to contact dep server: %r', e)
            raise DepServerError(e) from e


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
            return super(DepServerAptResolver, self).resolve_all(req)
        try:
            return asyncio.run(
                resolve_apt_requirement_dep_server(self.dep_server_url, req))
        except DepServerError:
            logging.warning('Falling back to resolving error locally')
            return super(DepServerAptResolver, self).resolve_all(req)

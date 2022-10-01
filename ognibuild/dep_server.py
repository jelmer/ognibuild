#!/usr/bin/python
# Copyright (C) 2022 Jelmer Vernooij <jelmer@jelmer.uk>
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

import sys

from aiohttp import web
from aiohttp_openmetrics import setup_metrics

from . import Requirement, UnknownRequirementFamily
from .debian.apt import AptManager
from .resolver.apt import resolve_requirement_apt


routes = web.RouteTableDef()


@routes.get('/health', name='health')
async def handle_health(request):
    return web.Response(text='ok')


@routes.get('/families', name='families')
async def handle_families(request):
    return web.json_response(list(Requirement._JSON_DESERIALIZERS.keys()))


@routes.post('/resolve-apt', name='resolve-apt')
async def handle_apt(request):
    js = await request.json()
    try:
        req = Requirement.from_json(js)
    except UnknownRequirementFamily as e:
        return web.json_response(
            {"reason": "family-unknown", "family": e.family}, status=404)
    apt_reqs = resolve_requirement_apt(request.app['apt_mgr'], req)
    return web.json_response([r.pkg_relation_str() for r in apt_reqs])


def main():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('--listen-address', type=str, help='Listen address')
    parser.add_argument('--schroot', type=str, help='Schroot session to use')
    parser.add_argument('--port', type=str, help='Listen port', default=9933)
    args = parser.parse_args()

    if args.schroot:
        from .session.schroot import SchrootSession
        session = SchrootSession(args.schroot)
    else:
        from .session.plain import PlainSession
        session = PlainSession()
    with session:
        app = web.Application()
        app.router.add_routes(routes)
        app['apt_mgr'] = AptManager.from_session(session)
        setup_metrics(app)

        web.run_app(app, host=args.listen_address, port=args.port)


if __name__ == '__main__':
    sys.exit(main())

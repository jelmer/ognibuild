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

import logging
import sys

from aiohttp import web
from aiohttp_openmetrics import setup_metrics

from . import Requirement, UnknownRequirementFamily
from .debian.apt import AptManager
from .resolver.apt import resolve_requirement_apt

SUPPORTED_RELEASES = ['unstable', 'sid']


routes = web.RouteTableDef()


@routes.get('/health', name='health')
async def handle_health(request):
    return web.Response(text='ok')


@routes.get('/ready', name='ready')
async def handle_ready(request):
    return web.Response(text='ok')


@routes.get('/families', name='families')
async def handle_families(request):
    return web.json_response(list(Requirement._JSON_DESERIALIZERS.keys()))


@routes.post('/resolve-apt', name='resolve-apt')
async def handle_apt(request):
    js = await request.json()
    try:
        req_js = js['requirement']
    except KeyError as e:
        raise web.HTTPBadRequest(text="json missing 'requirement' key") from e
    release = js.get('release')
    if release and release not in SUPPORTED_RELEASES:
        return web.json_response(
            {"reason": "unsupported-release", "release": release},
            headers={'Reason': 'unsupported-release'},
            status=404)
    try:
        req = Requirement.from_json(req_js)
    except UnknownRequirementFamily as e:
        return web.json_response(
            {"reason": "family-unknown", "family": e.family},
            headers={"Reason": 'unsupported-family'}, status=404)
    apt_reqs = await resolve_requirement_apt(request.app['apt_mgr'], req)
    return web.json_response([r.pkg_relation_str() for r in apt_reqs])


@routes.post('/resolve-apt/{release}/{family}', name='resolve-apt-new')
async def handle_apt_new(request):
    js = await request.json()
    js['family'] = request.match_info['family']
    release = request.match_info['release']
    if release not in SUPPORTED_RELEASES:
        return web.json_response(
            {"reason": "unsupported-release", "release": release},
            headers={'Reason': 'unsupported-release'},
            status=404)
    try:
        req = Requirement.from_json(js)
    except UnknownRequirementFamily as e:
        return web.json_response(
            {"reason": "family-unknown", "family": e.family},
            headers={"Reason": 'unsupported-family'}, status=404)
    apt_reqs = await resolve_requirement_apt(request.app['apt_mgr'], req)
    return web.json_response([r.pkg_relation_str() for r in apt_reqs])


@routes.get('/resolve-apt/{release}/{family}:{arg}', name='resolve-apt-simple')
async def handle_apt_simple(request):
    if request.match_info['release'] not in SUPPORTED_RELEASES:
        return web.json_response(
            {"reason": "unsupported-release",
             "release": request.match_info['release']},
            status=404)
    try:
        req = Requirement.from_json(
            (request.match_info['family'], request.match_info['arg']))
    except UnknownRequirementFamily as e:
        return web.json_response(
            {"reason": "family-unknown", "family": e.family}, status=404)
    apt_reqs = await resolve_requirement_apt(request.app['apt_mgr'], req)
    return web.json_response([r.pkg_relation_str() for r in apt_reqs])


def main():
    import argparse
    from .session import Session
    parser = argparse.ArgumentParser()
    parser.add_argument('--listen-address', type=str, help='Listen address')
    parser.add_argument('--schroot', type=str, help='Schroot session to use')
    parser.add_argument('--port', type=str, help='Listen port', default=9934)
    parser.add_argument('--debug', action='store_true')
    parser.add_argument(
        "--gcp-logging", action='store_true', help='Use Google cloud logging.')
    args = parser.parse_args()

    if args.gcp_logging:
        import google.cloud.logging
        client = google.cloud.logging.Client()
        client.get_default_handler()
        client.setup_logging()
    else:
        if args.debug:
            log_level = logging.DEBUG
        else:
            log_level = logging.INFO

        logging.basicConfig(
            level=log_level,
            format="[%(asctime)s] %(message)s",
            datefmt="%Y-%m-%d %H:%M:%S")

    session: Session
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

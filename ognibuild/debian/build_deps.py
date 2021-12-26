#!/usr/bin/python3
# Copyright (C) 2021 Jelmer Vernooij
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

"""Tie breaking by build deps."""


import logging


class NoAptSources(Exception):
    """No apt sources."""


class BuildDependencyTieBreaker(object):
    def __init__(self, rootdir):
        self.rootdir = rootdir
        self._counts = None

    def __repr__(self):
        return "%s(%r)" % (type(self).__name__, self.rootdir)

    @classmethod
    def from_session(cls, session):
        return cls(session.location)

    def _count(self):
        counts = {}
        import apt_pkg

        apt_pkg.init()
        apt_pkg.config.set("Dir", self.rootdir)
        try:
            apt_cache = apt_pkg.SourceRecords()
        except apt_pkg.Error as e:
            if (e.args[0] ==
                    "E:You must put some 'deb-src' URIs in your sources.list"):
                raise NoAptSources()
            raise
        apt_cache.restart()
        while apt_cache.step():
            try:
                for d in apt_cache.build_depends.values():
                    for o in d:
                        for p in o:
                            counts.setdefault(p[0], 0)
                            counts[p[0]] += 1
            except AttributeError:
                pass
        return counts

    def __call__(self, reqs):
        if self._counts is None:
            try:
                self._counts = self._count()
            except NoAptSources:
                logging.warning(
                    "No 'deb-src' in sources.list, "
                    "unable to break build-depends")
                return None
        by_count = {}
        for req in reqs:
            try:
                by_count[req] = self._counts[list(req.package_names())[0]]
            except KeyError:
                pass
        if not by_count:
            return None
        top = max(by_count.items(), key=lambda k: k[1])
        logging.info(
            "Breaking tie between [%s] to %s based on build-depends count",
            ', '.join([repr(r.pkg_relation_str()) for r in reqs]),
            repr(top[0].pkg_relation_str()),
        )
        return top[0]


if __name__ == "__main__":
    import argparse
    from ..resolver.apt import AptRequirement

    parser = argparse.ArgumentParser()
    parser.add_argument("req", nargs="+")
    args = parser.parse_args()
    reqs = [AptRequirement.from_str(req) for req in args.req]
    tie_breaker = BuildDependencyTieBreaker("/")
    print(tie_breaker(reqs))

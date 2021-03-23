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


class BuildDependencyTieBreaker(object):

    def __init__(self, rootdir):
        import apt_pkg
        apt_pkg.init()
        apt_pkg.config.set('Dir', rootdir)
        self._apt_cache = apt_pkg.SourceRecords()
        self._counts = None

    @classmethod
    def from_session(cls, session):
        return cls(session.location)

    def _count(self):
        counts = {}
        self._apt_cache.restart()
        while self._apt_cache.step():
            try:
                for d in self._apt_cache.build_depends.values():
                    for o in d:
                        for p in o:
                            counts.setdefault(p[0], 0)
                            counts[p[0]] += 1
            except AttributeError:
                pass
        return counts

    def __call__(self, reqs):
        if self._counts is None:
            self._counts = self._count()
        by_count = {}
        for req in reqs:
            by_count[req] = self._counts.get(list(req.package_names())[0])
        return max(by_count.items(), key=lambda k: k[1] or 0)[0]


if __name__ == '__main__':
    import argparse
    from ..resolver.apt import AptRequirement
    parser = argparse.ArgumentParser()
    parser.add_argument('req', nargs='+')
    args = parser.parse_args()
    reqs = [AptRequirement.from_str(req) for req in args.req]
    tie_breaker = BuildDependencyTieBreaker('/')
    print(tie_breaker(reqs))

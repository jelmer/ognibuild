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

"""Support for accessing UDD."""

import logging
from typing import Optional


class UDD:
    def connect(self):
        import psycopg2

        self._conn = psycopg2.connect(
            database="udd",
            user="udd-mirror",
            password="udd-mirror",
            port=5432,
            host="udd-mirror.debian.net",
        )

    def get_most_popular(self, packages) -> Optional[str]:
        cursor = self._conn.cursor()
        cursor.execute(
            "SELECT package FROM popcon "
            "WHERE package IN %s ORDER BY insts DESC LIMIT 1",
            (tuple(packages),),
        )
        row = cursor.fetchone()
        return row[0] if row else None


def popcon_tie_breaker(candidates):
    # TODO(jelmer): Pick package based on what appears most commonly in
    # build-depends{-indep,-arch}
    try:
        from .udd import UDD
    except ModuleNotFoundError:
        logging.warning("Unable to import UDD, not ranking by popcon")
        return sorted(candidates, key=len)[0]
    udd = UDD()
    udd.connect()
    names = {list(c.package_names())[0]: c for c in candidates}
    winner = udd.get_most_popular(list(names.keys()))
    if winner is None:
        logging.warning(
            "No relevant popcon information found, not ranking by popcon")
        return None
    logging.info("Picked winner using popcon")
    return names[winner]

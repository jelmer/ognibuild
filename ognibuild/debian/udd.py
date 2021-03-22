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


class UDD(object):

    def connect(self):
        import psycopg2

        self._conn = psycopg2.connect(
            database="udd",
            user="udd-mirror",
            password="udd-mirror",
            port=5432,
            host="udd-mirror.debian.net",
        )

    def get_most_popular(self, packages):
        cursor = self._conn.cursor()
        cursor.execute(
            'SELECT package FROM popcon WHERE package IN %s ORDER BY insts DESC LIMIT 1',
            (tuple(packages), ))
        return cursor.fetchone()[0]
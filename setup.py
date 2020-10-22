#!/usr/bin/env python3
# encoding: utf-8

from setuptools import setup


setup(name="ognibuild",
      description="Detect and run any build system",
      version="0.0.1",
      maintainer="Jelmer Vernooĳ",
      maintainer_email="jelmer@jelmer.uk",
      license="GNU GPLv2 or later",
      url="https://jelmer.uk/code/ognibuild",
      scripts=['ognibuild.py'],
      classifiers=[
          'Development Status :: 4 - Beta',
          'License :: OSI Approved :: '
          'GNU General Public License v2 or later (GPLv2+)',
          'Programming Language :: Python :: 3.5',
          'Programming Language :: Python :: 3.6',
          'Programming Language :: Python :: Implementation :: CPython',
          'Operating System :: POSIX',
      ])

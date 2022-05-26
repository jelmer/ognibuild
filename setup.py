#!/usr/bin/env python3
# encoding: utf-8

from setuptools import setup


setup(name="ognibuild",
      description="Detect and run any build system",
      version="0.0.14",
      maintainer="Jelmer Vernooĳ",
      maintainer_email="jelmer@jelmer.uk",
      license="GNU GPLv2 or later",
      url="https://jelmer.uk/code/ognibuild",
      packages=['ognibuild', 'ognibuild.tests', 'ognibuild.debian', 'ognibuild.resolver', 'ognibuild.session'],
      classifiers=[
          'Development Status :: 4 - Beta',
          'License :: OSI Approved :: '
          'GNU General Public License v2 or later (GPLv2+)',
          'Programming Language :: Python :: 3.5',
          'Programming Language :: Python :: 3.6',
          'Programming Language :: Python :: Implementation :: CPython',
          'Operating System :: POSIX',
      ],
      entry_points={
        "console_scripts": [
            "ogni=ognibuild.__main__:main",
            "deb-fix-build=ognibuild.debian.fix_build:main",
        ]
      },
      scripts=['scripts/report-apt-deps-status'],
      install_requires=[
          'breezy>=3.2',
          'buildlog-consultant>=0.0.21',
          'requirements-parser',
          ],
      extras_require={
          'debian': ['debmutate', 'python_debian', 'python_apt'],
          'remote': ['breezy', 'dulwich'],
      },
      tests_require=['python_debian', 'buildlog-consultant>=0.0.21', 'breezy', 'testtools'],
      test_suite='ognibuild.tests.test_suite',
      )

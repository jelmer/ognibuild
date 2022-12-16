#!/usr/bin/python
# Copyright (C) 2020 Jelmer Vernooij <jelmer@jelmer.uk>
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

import re

from debian.deb822 import Deb822

from buildlog_consultant.common import (
    MissingCommand,
    MissingGoPackage,
    MissingPerlModule,
    MissingPkgConfig,
    MissingPythonModule,
    MissingRubyFile,
    MissingRubyGem,
    MissingValaPackage,
)
from ognibuild.debian.apt import AptManager, FileSearcher
from ognibuild.debian.fix_build import (
    resolve_error,
    versioned_package_fixers,
    apt_fixers,
    DebianPackagingContext,
    add_build_dependency,
)
from ognibuild.resolver.apt import AptRequirement
from breezy.commit import NullCommitReporter
from breezy.tests import TestCaseWithTransport


class DummyAptSearcher(FileSearcher):
    def __init__(self, files):
        self._apt_files = files

    async def search_files(self, path, regex=False, case_insensitive=False):
        for p, pkg in sorted(self._apt_files.items()):
            flags: int
            if case_insensitive:
                flags = re.I
            else:
                flags = 0
            if regex:
                if re.match(path, p, flags):
                    yield pkg
            elif case_insensitive:
                if path.lower() == p.lower():
                    yield pkg
            else:
                if path == p:
                    yield pkg


class ResolveErrorTests(TestCaseWithTransport):
    def setUp(self):
        super(ResolveErrorTests, self).setUp()
        self.tree = self.make_branch_and_tree(".")
        self.build_tree_contents(
            [
                ("debian/",),
                (
                    "debian/control",
                    """\
Source: blah
Build-Depends: libc6

Package: python-blah
Depends: ${python3:Depends}
Description: A python package
 Foo
""",
                ),
                (
                    "debian/changelog",
                    """\
blah (0.1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #XXXXXX)

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 04 Apr 2020 14:12:13 +0000
""",
                ),
            ]
        )
        self.tree.add(["debian", "debian/control", "debian/changelog"])
        self.tree.commit("Initial commit")
        self._apt_files = {}

    def resolve(self, error, context=("build",)):
        from ognibuild.session.plain import PlainSession

        session = PlainSession()
        apt = AptManager(session)
        apt._searchers = [DummyAptSearcher(self._apt_files)]
        context = DebianPackagingContext(
            self.tree,
            subpath="",
            committer="ognibuild <ognibuild@jelmer.uk>",
            update_changelog=True,
            commit_reporter=NullCommitReporter(),
        )
        fixers = versioned_package_fixers(
            session, context, apt) + apt_fixers(apt, context)
        return resolve_error(error, ("build",), fixers)

    def get_build_deps(self):
        with open(self.tree.abspath("debian/control"), "r") as f:
            return next(Deb822.iter_paragraphs(f)).get("Build-Depends", "")

    def test_missing_command_unknown(self):
        self._apt_files = {}
        self.assertFalse(self.resolve(
            MissingCommand("acommandthatdoesnotexist")))

    def test_missing_command_brz(self):
        self._apt_files = {
            "/usr/bin/b": "bash",
            "/usr/bin/brz": "brz",
            "/usr/bin/brzier": "bash",
        }
        self.overrideEnv("DEBEMAIL", "jelmer@debian.org")
        self.overrideEnv("DEBFULLNAME", "Jelmer Vernooĳ")
        self.assertTrue(self.resolve(MissingCommand("brz")))
        self.assertEqual("libc6, brz", self.get_build_deps())
        rev = self.tree.branch.repository.get_revision(
            self.tree.branch.last_revision())
        self.assertEqual("Add missing build dependency on brz.\n", rev.message)
        self.assertFalse(self.resolve(MissingCommand("brz")))
        self.assertEqual("libc6, brz", self.get_build_deps())

    def test_missing_command_ps(self):
        self._apt_files = {
            "/bin/ps": "procps",
            "/usr/bin/pscal": "xcal",
        }
        self.assertTrue(self.resolve(MissingCommand("ps")))
        self.assertEqual("libc6, procps", self.get_build_deps())

    def test_missing_ruby_file(self):
        self._apt_files = {
            "/usr/lib/ruby/vendor_ruby/rake/testtask.rb": "rake",
        }
        self.assertTrue(self.resolve(MissingRubyFile("rake/testtask")))
        self.assertEqual("libc6, rake", self.get_build_deps())

    def test_missing_ruby_file_from_gem(self):
        self._apt_files = {
            "/usr/share/rubygems-integration/all/gems/activesupport-"
            "5.2.3/lib/active_support/core_ext/string/strip.rb":
                "ruby-activesupport"
        }
        self.assertTrue(
            self.resolve(MissingRubyFile(
                "active_support/core_ext/string/strip"))
        )
        self.assertEqual("libc6, ruby-activesupport", self.get_build_deps())

    def test_missing_ruby_gem(self):
        self._apt_files = {
            "/usr/share/rubygems-integration/all/specifications/"
            "bio-1.5.2.gemspec": "ruby-bio",
            "/usr/share/rubygems-integration/all/specifications/"
            "bio-2.0.2.gemspec": "ruby-bio",
        }
        self.assertTrue(self.resolve(MissingRubyGem("bio", None)))
        self.assertEqual("libc6, ruby-bio", self.get_build_deps())
        self.assertTrue(self.resolve(MissingRubyGem("bio", "2.0.3")))
        self.assertEqual("libc6, ruby-bio (>= 2.0.3)", self.get_build_deps())

    def test_missing_perl_module(self):
        self._apt_files = {
            "/usr/share/perl5/App/cpanminus/fatscript.pm": "cpanminus"}
        self.assertTrue(
            self.resolve(
                MissingPerlModule(
                    "App/cpanminus/fatscript.pm",
                    "App::cpanminus::fatscript",
                    [
                        "/<<PKGBUILDDIR>>/blib/lib",
                        "/<<PKGBUILDDIR>>/blib/arch",
                        "/etc/perl",
                        "/usr/local/lib/x86_64-linux-gnu/perl/5.30.0",
                        "/usr/local/share/perl/5.30.0",
                        "/usr/lib/x86_64-linux-gnu/perl5/5.30",
                        "/usr/share/perl5",
                        "/usr/lib/x86_64-linux-gnu/perl/5.30",
                        "/usr/share/perl/5.30",
                        "/usr/local/lib/site_perl",
                        "/usr/lib/x86_64-linux-gnu/perl-base",
                        ".",
                    ],
                )
            )
        )
        self.assertEqual("libc6, cpanminus", self.get_build_deps())

    def test_missing_pkg_config(self):
        self._apt_files = {
            "/usr/lib/x86_64-linux-gnu/pkgconfig/xcb-xfixes.pc":
                "libxcb-xfixes0-dev"
        }
        self.assertTrue(self.resolve(MissingPkgConfig("xcb-xfixes")))
        self.assertEqual("libc6, libxcb-xfixes0-dev", self.get_build_deps())

    def test_missing_pkg_config_versioned(self):
        self._apt_files = {
            "/usr/lib/x86_64-linux-gnu/pkgconfig/xcb-xfixes.pc":
                "libxcb-xfixes0-dev"
        }
        self.assertTrue(self.resolve(MissingPkgConfig("xcb-xfixes", "1.0")))
        self.assertEqual(
            "libc6, libxcb-xfixes0-dev (>= 1.0)", self.get_build_deps())

    def test_missing_python_module(self):
        self._apt_files = {
            "/usr/lib/python3/dist-packages/m2r.py": "python3-m2r"}
        self.assertTrue(self.resolve(MissingPythonModule("m2r")))
        self.assertEqual("libc6, python3-m2r", self.get_build_deps())

    def test_missing_go_package(self):
        self._apt_files = {
            "/usr/share/gocode/src/github.com/chzyer/readline/utils_test.go":
                "golang-github-chzyer-readline-dev",
        }
        self.assertTrue(self.resolve(
            MissingGoPackage("github.com/chzyer/readline")))
        self.assertEqual(
            "libc6, golang-github-chzyer-readline-dev", self.get_build_deps()
        )

    def test_missing_vala_package(self):
        self._apt_files = {
            "/usr/share/vala-0.48/vapi/posix.vapi": "valac-0.48-vapi",
        }
        self.assertTrue(self.resolve(MissingValaPackage("posix")))
        self.assertEqual("libc6, valac-0.48-vapi", self.get_build_deps())


class AddBuildDependencyTests(TestCaseWithTransport):

    def setUp(self):
        super(AddBuildDependencyTests, self).setUp()
        self.tree = self.make_branch_and_tree(".")
        self.build_tree_contents(
            [
                ("debian/",),
                (
                    "debian/control",
                    """\
Source: blah
Build-Depends: libc6

Package: python-blah
Depends: ${python3:Depends}
Description: A python package
 Foo
""",
                ),
                (
                    "debian/changelog",
                    """\
blah (0.1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #XXXXXX)

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 04 Apr 2020 14:12:13 +0000
""",
                ),
            ]
        )
        self.tree.add(["debian", "debian/control", "debian/changelog"])
        self.tree.commit("Initial commit")
        self.context = DebianPackagingContext(
            self.tree,
            subpath="",
            committer="ognibuild <ognibuild@jelmer.uk>",
            update_changelog=True,
            commit_reporter=NullCommitReporter(),
        )

    def test_already_present(self):
        requirement = AptRequirement.simple('libc6')
        self.assertFalse(add_build_dependency(self.context, requirement))

    def test_basic(self):
        requirement = AptRequirement.simple('foo')
        self.assertTrue(add_build_dependency(self.context, requirement))
        self.assertFileEqual("""\
Source: blah
Build-Depends: libc6, foo

Package: python-blah
Depends: ${python3:Depends}
Description: A python package
 Foo
""", 'debian/control')

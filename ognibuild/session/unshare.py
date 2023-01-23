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

from contextlib import ExitStack
import logging
import os
import pwd
import subprocess
import tempfile

from typing import Optional


from . import Session, NoSessionOpen, SessionAlreadyOpen


TARBALL_EXCLUDE_FILES = [
	"/dev/urandom",
	"/dev/random",
	"/dev/full",
	"/dev/null",
	"/dev/console",
	"/dev/zero",
	"/dev/tty",
	"/dev/ptmx",
]


class UnshareSession(Session):

    path: Optional[str]

    # Currently working directory inside the chroot
    _cwd: Optional[str]

    def __init__(self, setup_fn, *, name=None):
        self.es = ExitStack()
        self.name = name
        self._setup_fn = setup_fn
        self.path = None

    @classmethod
    def from_tarball(cls, tarball_path, *, name=None):
        def setup(es, path):
            import tarfile
            tf = tarfile.open(tarball_path, mode='r')

            def makedev(info, path):
                if info.name.lstrip('.') not in TARBALL_EXCLUDE_FILES:
                    logging.warning('Not creating dev node %s', info.name)
            tf.makedev = makedev
            tf.extractall(path)
            with open(os.path.join(path, 'etc/passwd'), 'a') as f:
                f.write(':'.join(map(str, pwd.getpwuid(os.getuid()))) + '\n')
        return cls(setup, name=name)

    def __enter__(self) -> "UnshareSession":
        if self.path is not None:
            raise SessionAlreadyOpen(self)
        self.es.__enter__()
        self.path = self.es.enter_context(
            tempfile.TemporaryDirectory(prefix=self.name))
        self._setup_fn(self.es, self.path)
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        if self.path is None:
            raise NoSessionOpen(self)
        self.path = None
        return self.es.__exit__(exc_type, exc_val, exc_tb)

    def chdir(self, cwd: str) -> None:
        self._cwd = cwd

    def _unshare_argv(self, argv, user=None, cwd=None):
        if self.path is None:
            raise NoSessionOpen(self)
        unshare_args = [f"--root={self.path}"]
        unshare_args.append(f"--map-users=auto")
        if user == "root":
            unshare_args.append("--map-root-user")
        elif user is None or user == os.environ["LOGNAME"]:
            unshare_args.append("--map-current-user")
        else:
            raise ValueError(f"unsupported user {user}")
        unshare_args.extend([
            "--cgroup",
            "--user",
            "--pid",
            "--uts",
            "--mount",
            "--ipc",
            "--fork",
            "--mount-proc",
            "--map-groups=auto",
            "--kill-child",
        ])
        if cwd or self._cwd:
            unshare_args.append(f"--wd={cwd or self._cwd}")
        return ["unshare"] + unshare_args + ["--"] + argv

    @property
    def location(self) -> str:
        if self.path is None:
            raise NoSessionOpen(self)
        return self.path

    def check_call(
        self,
        argv: list[str],
        cwd: Optional[str] = None,
        user: Optional[str] = None,
        env: Optional[dict[str, str]] = None,
        close_fds: bool = True,
    ):
        return subprocess.check_call(
            self._unshare_argv(argv, user=user, cwd=cwd),
            env=env,
            close_fds=close_fds)

    def check_output(
        self,
        argv: list[str],
        cwd: Optional[str] = None,
        user: Optional[str] = None,
        env: Optional[dict[str, str]] = None,
    ) -> bytes:
        return subprocess.check_output(
            self._unshare_argv(argv, user=user, cwd=cwd),
            env=env)

    def Popen(
        self, argv, cwd: Optional[str] = None, user: Optional[str] = None,
        **kwargs
    ):
        return subprocess.Popen(
            self._unshare_argv(argv, user=user, cwd=cwd),
            **kwargs)

    def call(
        self, argv: list[str], cwd: Optional[str] = None,
        user: Optional[str] = None
    ):
        return subprocess.call(self._unshare_argv(argv, user=user, cwd=cwd))

    def external_path(self, path: str) -> str:
        if os.path.isabs(path):
            return os.path.join(self.path, path.lstrip("/"))
        if self._cwd is None:
            raise ValueError("no cwd set")
        return os.path.join(
            self.path, os.path.join(self._cwd, path).lstrip("/"))

    def create_home(self) -> None:
        """Create the user's home directory."""
        import pdb; pdb.set_trace()
        home = (
            self.check_output(
                ["sh", "-c", "echo $HOME"], cwd="/").decode().rstrip("\n")
        )
        user = (
            self.check_output(["sh", "-c", "echo $LOGNAME"], cwd="/")
            .decode()
            .rstrip("\n")
        )
        logging.info("Creating directory %s in unshare session.", home)
        self.check_call(["mkdir", "-p", home], cwd="/", user="root")
        self.check_call(["chown", user, home], cwd="/", user="root")

    def exists(self, path: str) -> bool:
        """Check whether a path exists in the chroot."""
        fullpath = self.external_path(path)
        return os.path.exists(fullpath)

    def scandir(self, path: str):
        fullpath = self.external_path(path)
        return os.scandir(fullpath)

    def mkdir(self, path: str):
        fullpath = self.external_path(path)
        return os.mkdir(fullpath)

    def rmtree(self, path: str):
        import shutil
        fullpath = self.external_path(path)
        return shutil.rmtree(fullpath)

    is_temporary = True

    def setup_from_vcs(
        self, tree, include_controldir: Optional[bool] = None, subdir="package"
    ):
        from ..vcs import dupe_vcs_tree, export_vcs_tree

        build_dir = os.path.join(self.path, "build")
        os.makedirs(build_dir, exist_ok=True)
        directory = tempfile.mkdtemp(dir=build_dir)
        reldir = "/" + os.path.relpath(directory, self.path)

        export_directory = os.path.join(directory, subdir)
        if not include_controldir:
            export_vcs_tree(tree, export_directory)
        else:
            dupe_vcs_tree(tree, export_directory)

        return export_directory, os.path.join(reldir, subdir)

    def setup_from_directory(self, path, subdir="package"):
        import shutil

        build_dir = os.path.join(self.path, "build")
        os.makedirs(build_dir, exist_ok=True)
        directory = tempfile.mkdtemp(dir=build_dir)
        reldir = "/" + os.path.relpath(directory, self.path)
        export_directory = os.path.join(directory, subdir)
        shutil.copytree(path, export_directory, symlinks=True)
        return export_directory, os.path.join(reldir, subdir)

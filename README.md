ognibuild
=========

Ognibuild is a simple wrapper with a common interface for invoking any kind of
build tool.

The ideas is that it can be run to build and install any source code directory
by detecting the build system that is in use and invoking that with the correct
parameters.

It can also detect and install missing dependencies.

Goals
-----

The goal of ognibuild is to provide a consistent CLI that can be used for any
software package. It is mostly useful for automated building of
large sets of diverse packages (e.g. different programming languages).

It is not meant to expose all functionality that is present in the underlying
build systems. To use that, invoke those build systems directly.

Usage
-----

Ognibuild has a number of subcommands:

 * ``ogni clean`` - remove any built artifacts
 * ``ogni dist`` - create a source tarball
 * ``ogni build`` - build the package in-tree
 * ``ogni install`` - install the package
 * ``ogni test`` - run the testsuite in the source directory

It also includes a subcommand that can fix up the build dependencies
for Debian packages, called deb-fix-build.

License
-------

Ognibuild is licensed under the GNU GPL, v2 or later.

# ognibuild

Ognibuild is a simple wrapper with a common interface for invoking any kind of
build tool.

The ideas is that it can be run to build and install any source code directory
by detecting the build system that is in use and invoking that with the correct
parameters.

It can also detect and install missing dependencies.

## Goals

The goal of ognibuild is to provide a consistent CLI that can be used for any
software package. It is mostly useful for automated building of
large sets of diverse packages (e.g. different programming languages).

It is not meant to expose all functionality that is present in the underlying
build systems. To use that, invoke those build systems directly.

## Usage

Ognibuild has a number of subcommands:

 * ``ogni clean`` - remove any built artifacts
 * ``ogni dist`` - create a source tarball
 * ``ogni build`` - build the package in-tree
 * ``ogni install`` - install the package
 * ``ogni test`` - run the testsuite in the source directory

It also includes a subcommand that can fix up the build dependencies
for Debian packages, called deb-fix-build.

### Examples

```
ogni -d https://gitlab.gnome.org/GNOME/fractal install
```

## Status

Ognibuild is functional, but sometimes rough around the edges. If you run into
issues (or lack of support for a particular ecosystem), please file a bug.

### Supported Build Systems

- Bazel
- Cabal
- Cargo
- Golang
- Gradle
- Make, including various makefile generators:
    - autoconf/automake
    - CMake
    - Makefile.PL
    - qmake
- Maven
- ninja, including ninja file generators:
    - meson
- Node
- Octave
- Perl
    - Module::Build::Tiny
    - Dist::Zilla
    - Minilla
- PHP Pear
- Python - setup.py/setup.cfg/pyproject.toml
- R
- Ruby gems
- Waf

### Supported package repositories

Package repositories are used to install missing dependencies.

The following "native" repositories are supported:

- pypi
- cpan
- hackage
- npm
- cargo
- cran
- golang\*

As well one distribution repository:

- apt

## License

Ognibuild is licensed under the GNU GPL, v2 or later.

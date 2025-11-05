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
 * ``ogni cache-env`` - cache a Debian cloud image for testing (Linux only)

It also includes a subcommand that can fix up the build dependencies
for Debian packages, called deb-fix-build.

### Examples

```
ogni -d https://gitlab.gnome.org/GNOME/fractal install
```

### Caching Test Environments

On Linux systems with the `debian` feature enabled, ognibuild can use cached Debian cloud images
to speed up tests that require a Debian environment. This is particularly useful for running tests
with `UnshareSession`.

To cache a Debian image:

```bash
ogni cache-env sid
```

Once cached, tests will automatically use the cached image instead of bootstrapping a new environment
from the network. This significantly reduces test execution time.

You can also specify a different Debian suite:

```bash
ogni cache-env bookworm
ogni cache-env stable
```

The cached images are stored in `~/.cache/ognibuild/images/`.

To run tests without network access:

1. First, cache an image and build everything (requires network):
   ```bash
   ogni cache-env sid
   cargo build --all-targets
   ```

2. Run tests in a network-isolated environment (requires sudo):
   ```bash
   sudo CARGO_HOME=$HOME/.cargo OGNIBUILD_DEBIAN_TEST_TARBALL=$HOME/.cache/ognibuild/images/debian-sid-amd64.tar.gz unshare -n cargo test --frozen
   ```

The `CARGO_HOME` environment variable ensures cargo finds the downloaded dependencies.
The `OGNIBUILD_DEBIAN_TEST_TARBALL` environment variable points to the cached Debian image.
The `--frozen` flag prevents cargo from accessing the network.

### Environment Variables

Ognibuild respects the following environment variables:

- `OGNIBUILD_DISABLE_NET` - When set to `1`, `true`, `yes`, or `on` (case-insensitive), prevents the `ogni cache-env` CLI command from downloading Debian images. Note: This only affects the CLI tool, not library code.
- `OGNIBUILD_DEPS` - URL of the ognibuild dependency server to use for resolving dependencies.
- `OGNIBUILD_DEBIAN_TEST_TARBALL` - Path to a custom Debian tarball to use for testing instead of downloading one.

### Running Tests Without Network Access

To run tests in a network-isolated environment:

1. First, cache a Debian image (requires network):
   ```bash
   ogni cache-env sid
   ```

2. Then run tests in a network namespace (requires root or CAP_NET_ADMIN):
   ```bash
   sudo unshare -n -- sudo -u $USER bash -c 'cd $(pwd) && cargo test'
   ```

If no cached image exists and network is unavailable, tests will fail with a clear error message indicating that either a cached image or network access is required.

### Debugging

If you run into any issues, please see [Debugging](notes/debugging.md).

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

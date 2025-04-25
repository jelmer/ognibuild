#![deny(missing_docs)]
//! Library for building packages from source code.

/// Action implementations like build, clean, test, etc.
pub mod actions;
/// Analyze build errors and execution problems.
pub mod analyze;
/// Build log handling and parsing.
pub mod buildlog;
/// BuildSystem trait and related types.
pub mod buildsystem;
/// Implementations of different build systems.
pub mod buildsystems;
#[cfg(feature = "debian")]
/// Debian-specific functionality.
pub mod debian;
/// Dependency resolution implementations.
pub mod dependencies;
/// Dependency trait and related types.
pub mod dependency;
/// Distribution package creation.
pub mod dist;
/// Utilities for catching distribution packages.
pub mod dist_catcher;
/// Build fixing utilities.
pub mod fix_build;
/// Implementations of different build fixers.
pub mod fixers;
/// Package installer functionality.
pub mod installer;
/// Logging utilities.
pub mod logs;
/// Output formatting and handling.
pub mod output;
/// Session handling for build environments.
pub mod session;
/// Shebang detection and processing.
pub mod shebang;
#[cfg(feature = "upstream")]
/// Upstream package handling.
pub mod upstream;
#[cfg(feature = "breezy")]
/// Version control system utilities.
pub mod vcs;

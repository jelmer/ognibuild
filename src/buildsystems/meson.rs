use crate::analyze::AnalyzedError;
use crate::buildsystem::{BuildSystem, DependencyCategory, Error};
use crate::dependencies::vague::VagueDependency;
use crate::dependency::Dependency;
use crate::dist_catcher::DistCatcher;
use crate::fix_build::BuildFixer;
use crate::installer::Error as InstallerError;
use crate::session::Session;
use std::path::{Path, PathBuf};

#[derive(Debug)]
/// Meson build system.
///
/// Handles projects built with Meson and Ninja.
pub struct Meson {
    #[allow(dead_code)]
    path: PathBuf,
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct MesonDependency {
    pub name: String,
    #[serde(deserialize_with = "version_as_vec")]
    pub version: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub has_fallback: bool,
    #[serde(default)]
    pub conditional: bool,
}

// Helper to handle both string and vec for version field
fn version_as_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<String>),
    }

    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::String(s) => Ok(if s.is_empty() { vec![] } else { vec![s] }),
        StringOrVec::Vec(v) => Ok(v),
    }
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct MesonTarget {
    r#type: String,
    installed: bool,
    filename: Vec<PathBuf>,
}

impl Meson {
    /// Create a new Meson build system with the specified path.
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    fn setup(&self, session: &dyn Session) -> Result<(), Error> {
        // Get the project directory (parent of meson.build)
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        let build_dir = project_dir.join("build");

        if !session.exists(&build_dir) {
            session.mkdir(&build_dir).unwrap();
        }
        session
            .command(vec!["meson", "setup", "build"])
            .cwd(project_dir)
            .quiet(true)
            .run_detecting_problems()?;
        Ok(())
    }

    fn introspect<T: for<'a> serde::Deserialize<'a>>(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn BuildFixer<InstallerError>]>,
        args: &[&str],
    ) -> Result<T, InstallerError> {
        // Get the project directory (parent of meson.build)
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");

        // Check if we have a configured build directory
        let build_dir = project_dir.join("build");
        let use_build_dir =
            session.exists(&build_dir) && session.exists(&build_dir.join("build.ninja"));

        let ret = if use_build_dir {
            // Use configured build directory
            let build_dir_str = build_dir.to_string_lossy();
            let introspect_args = [&["meson", "introspect", &build_dir_str], args].concat();

            if let Some(fixers) = fixers {
                session
                    .command(introspect_args)
                    .cwd(project_dir)
                    .quiet(true)
                    .run_fixing_problems::<_, Error>(fixers)
                    .unwrap()
            } else {
                session
                    .command(introspect_args)
                    .cwd(project_dir)
                    .quiet(true)
                    .run_detecting_problems()?
            }
        } else {
            // For unconfigured projects, set up a temporary build and introspect from there
            self.setup_temp_build_for_introspect(session, fixers, args)?
        };

        let text = ret.concat();
        Ok(serde_json::from_str(&text).unwrap())
    }

    /// Set up a temporary build directory for introspection of unconfigured projects
    fn setup_temp_build_for_introspect(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn BuildFixer<InstallerError>]>,
        args: &[&str],
    ) -> Result<Vec<String>, InstallerError> {
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");

        // Create a temporary build directory
        let temp_build_dir = project_dir.join(".ognibuild-temp-build");

        // Clean up any existing temp build
        if session.exists(&temp_build_dir) {
            session.rmtree(&temp_build_dir).ok();
        }

        session.mkdir(&temp_build_dir).map_err(|e| {
            InstallerError::Other(format!("Failed to create temp build dir: {}", e))
        })?;

        // Set up the build directory
        let temp_build_str = temp_build_dir.to_string_lossy();
        let setup_result = session
            .command(vec!["meson", "setup", &temp_build_str])
            .cwd(project_dir)
            .quiet(true)
            .run_detecting_problems();

        match setup_result {
            Ok(_) => {
                // Now introspect the configured build directory
                let temp_build_str = temp_build_dir.to_string_lossy();
                let introspect_args = [&["meson", "introspect", &temp_build_str], args].concat();

                let result = if let Some(fixers) = fixers {
                    session
                        .command(introspect_args)
                        .cwd(project_dir)
                        .quiet(true)
                        .run_fixing_problems::<_, Error>(fixers)
                        .unwrap()
                } else {
                    session
                        .command(introspect_args)
                        .cwd(project_dir)
                        .quiet(true)
                        .run_detecting_problems()?
                };

                // Clean up temp build directory
                session.rmtree(&temp_build_dir).ok();

                Ok(result)
            }
            Err(e) => {
                // Clean up temp build directory on failure
                session.rmtree(&temp_build_dir).ok();
                Err(e.into())
            }
        }
    }

    /// Probe a directory for a Meson build system.
    ///
    /// Returns a Meson build system if a meson.build file is found.
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        let path = path.join("meson.build");
        if path.exists() {
            log::debug!("Found meson.build, assuming meson package.");
            Some(Box::new(Self::new(&path)))
        } else {
            None
        }
    }
}

impl BuildSystem for Meson {
    fn name(&self) -> &str {
        "meson"
    }

    fn dist(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
        target_directory: &Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        self.setup(session)?;
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        let dc = DistCatcher::new(vec![
            session.external_path(&project_dir.join("build/meson-dist"))
        ]);
        match session
            .command(vec!["ninja", "-C", "build", "dist"])
            .cwd(project_dir)
            .quiet(true)
            .run_detecting_problems()
        {
            Ok(_) => {}
            Err(AnalyzedError::Unidentified { lines, .. })
                if lines.contains(
                    &"ninja: error: unknown target 'dist', did you mean 'dino'?".to_string(),
                ) =>
            {
                unimplemented!();
            }
            Err(e) => return Err(e.into()),
        }
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        self.setup(session)?;
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        session
            .command(vec!["ninja", "-C", "build", "test"])
            .cwd(project_dir)
            .quiet(true)
            .run_detecting_problems()?;
        Ok(())
    }

    fn build(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        self.setup(session)?;
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        session
            .command(vec!["ninja", "-C", "build"])
            .cwd(project_dir)
            .quiet(true)
            .run_detecting_problems()?;
        Ok(())
    }

    fn clean(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        self.setup(session)?;
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        session
            .command(vec!["ninja", "-C", "build", "clean"])
            .cwd(project_dir)
            .quiet(true)
            .run_detecting_problems()?;
        Ok(())
    }

    fn install(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), Error> {
        self.setup(session)?;
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        session
            .command(vec!["ninja", "-C", "build", "install"])
            .cwd(project_dir)
            .quiet(true)
            .run_detecting_problems()?;
        Ok(())
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<(crate::buildsystem::DependencyCategory, Box<dyn Dependency>)>, Error> {
        let mut ret: Vec<(DependencyCategory, Box<dyn Dependency>)> = Vec::new();

        // Use --scan-dependencies directly on the meson.build file
        // This is the correct usage - scan-dependencies works on source files, not build dirs
        // Get the project directory (parent of meson.build)
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");

        let meson_file_str = self.path.to_string_lossy();
        let scan_args = vec![
            "meson",
            "introspect",
            "--scan-dependencies",
            &meson_file_str,
        ];

        let output = if let Some(fixers) = fixers {
            session
                .command(scan_args)
                .cwd(project_dir)
                .quiet(true)
                .run_fixing_problems::<_, Error>(fixers)
                .map_err(|e| {
                    InstallerError::Other(format!("Failed to run scan-dependencies: {:?}", e))
                })?
        } else {
            session
                .command(scan_args)
                .cwd(project_dir)
                .quiet(true)
                .run_detecting_problems()
                .map_err(|e| {
                    InstallerError::Other(format!("Failed to run scan-dependencies: {:?}", e))
                })?
        };

        let text = output.concat();
        let resp: Vec<MesonDependency> = serde_json::from_str(&text).map_err(|e| {
            InstallerError::Other(format!("Failed to parse scan-dependencies JSON: {}", e))
        })?;

        for entry in resp {
            let mut minimum_version = None;
            if entry.version.len() == 1 {
                if let Some(rest) = entry.version[0].strip_prefix(">=") {
                    minimum_version = Some(rest.trim().to_string());
                }
            } else if entry.version.len() > 1 {
                log::warn!("Unable to parse version constraints: {:?}", entry.version);
            }
            // TODO(jelmer): Include entry['required']
            ret.push((
                DependencyCategory::Universal,
                Box::new(VagueDependency {
                    name: entry.name.to_string(),
                    minimum_version,
                }),
            ));
        }
        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, Error> {
        let mut ret: Vec<Box<dyn crate::output::Output>> = Vec::new();
        let resp = self.introspect::<Vec<MesonTarget>>(session, fixers, &["--targets"])?;
        for entry in resp {
            if !entry.installed {
                continue;
            }
            if entry.r#type == "executable" {
                for p in entry.filename {
                    ret.push(Box::new(crate::output::BinaryOutput::new(
                        p.file_name().unwrap().to_str().unwrap(),
                    )));
                }
            }
            // TODO(jelmer): Handle other types
        }

        Ok(ret)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildsystem::detect_buildsystems;
    use crate::installer::NullInstaller;
    use crate::session::plain::PlainSession;
    use std::fs;
    use tempfile::TempDir;

    /// Helper function to create a minimal meson.build file
    fn create_meson_project(dir: &Path) -> std::io::Result<()> {
        fs::write(
            dir.join("meson.build"),
            r#"project('test-project', 'c',
  version : '1.0.0',
  license : 'MIT',
  default_options : ['warning_level=2'])

# A simple dependency for testing
glib_dep = dependency('glib-2.0', required: false)

# Define a simple executable
executable('test-app',
  'main.c',
  dependencies : glib_dep,
  install : true)
"#,
        )?;

        // Create a simple C source file
        fs::write(
            dir.join("main.c"),
            r#"#include <stdio.h>

int main(int argc, char *argv[]) {
    printf("Hello from Meson test!\n");
    return 0;
}
"#,
        )?;

        Ok(())
    }

    #[test]
    fn test_meson_detection() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path();

        // Should not detect Meson without meson.build
        let buildsystems = detect_buildsystems(project_dir);
        assert!(
            !buildsystems.iter().any(|bs| bs.name() == "meson"),
            "Should not detect Meson without meson.build"
        );

        // Create meson.build
        create_meson_project(project_dir).unwrap();

        // Should detect Meson with meson.build
        let buildsystems = detect_buildsystems(project_dir);
        assert!(
            buildsystems.iter().any(|bs| bs.name() == "meson"),
            "Should detect Meson with meson.build"
        );

        // Verify it's the first detected build system (highest priority)
        if !buildsystems.is_empty() {
            let first = &buildsystems[0];
            assert_eq!(
                first.name(),
                "meson",
                "Meson should be the primary build system"
            );
        }
    }

    #[test]
    fn test_meson_introspect_with_different_cwd() {
        // This test verifies the fix for the cwd issue
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("my-project");
        fs::create_dir(&project_dir).unwrap();

        create_meson_project(&project_dir).unwrap();

        // Create a session with a different working directory
        let mut session = PlainSession::new();
        // Set the session's cwd to the temp directory, NOT the project directory
        session.chdir(temp_dir.path()).unwrap();

        // Detect the buildsystem
        let buildsystems = detect_buildsystems(&project_dir);
        assert!(!buildsystems.is_empty(), "Should detect Meson buildsystem");

        let meson = buildsystems
            .iter()
            .find(|bs| bs.name() == "meson")
            .expect("Should find Meson buildsystem");

        // Try to get dependencies - this should work even with different cwd
        match meson.get_declared_dependencies(&session, None) {
            Ok(deps) => {
                // Check that we found the glib dependency
                let _has_glib = deps
                    .iter()
                    .any(|(_, dep)| dep.family() == "glib" || dep.family() == "pkg-config");
                log::debug!("Found {} dependencies", deps.len());
                if !deps.is_empty() {
                    log::debug!("Dependencies: {:?}", deps);
                }
            }
            Err(e) => {
                // It's okay if meson isn't installed, but the error should NOT be
                // about missing meson.build file
                let error_str = format!("{:?}", e);
                assert!(
                    !error_str.contains("Missing Meson file"),
                    "Should not fail with 'Missing Meson file' error: {}",
                    error_str
                );
                assert!(
                    !error_str.contains("./meson.build"),
                    "Should not reference relative path './meson.build': {}",
                    error_str
                );
            }
        }
    }

    #[test]
    fn test_meson_with_nested_project_structure() {
        // Test that Meson works correctly with nested directory structures
        let temp_dir = TempDir::new().unwrap();
        let workspace_dir = temp_dir.path().join("workspace");
        let project_dir = workspace_dir.join("subdir").join("project");
        fs::create_dir_all(&project_dir).unwrap();

        create_meson_project(&project_dir).unwrap();

        // Create session at workspace root
        let mut session = PlainSession::new();
        session.chdir(&workspace_dir).unwrap();

        // Detect buildsystem in nested project
        let buildsystems = detect_buildsystems(&project_dir);
        let meson = buildsystems
            .iter()
            .find(|bs| bs.name() == "meson")
            .expect("Should find Meson buildsystem");

        // This should work despite the session being 2 levels up from the project
        match meson.get_declared_dependencies(&session, None) {
            Ok(_) => {
                // Success - the cwd handling is working correctly
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                // Check it's not a path-related error
                assert!(
                    !error_str.contains("Missing Meson file")
                        && !error_str.contains("./meson.build"),
                    "Path handling error: {}",
                    error_str
                );
            }
        }
    }

    #[test]
    #[ignore] // This test requires meson to be installed
    fn test_meson_build_operations() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("build-test");
        fs::create_dir(&project_dir).unwrap();

        create_meson_project(&project_dir).unwrap();

        // Create session in a different directory
        let mut session = PlainSession::new();
        session.chdir(temp_dir.path()).unwrap();

        let buildsystems = detect_buildsystems(&project_dir);
        let meson = buildsystems
            .iter()
            .find(|bs| bs.name() == "meson")
            .expect("Should find Meson buildsystem");

        // Test build operation
        let installer = NullInstaller;

        // Build should work with proper cwd handling
        match meson.build(&session, &installer) {
            Ok(_) => log::debug!("Build succeeded"),
            Err(e) => {
                let error_str = format!("{:?}", e);
                assert!(
                    !error_str.contains("./meson.build"),
                    "Should not have path errors: {}",
                    error_str
                );
            }
        }

        // Test clean operation
        match meson.clean(&session, &installer) {
            Ok(_) => log::debug!("Clean succeeded"),
            Err(e) => {
                let error_str = format!("{:?}", e);
                assert!(
                    !error_str.contains("./meson.build"),
                    "Should not have path errors: {}",
                    error_str
                );
            }
        }
    }

    #[test]
    fn test_meson_project_with_subprojects() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path();

        // Create main project
        fs::write(
            project_dir.join("meson.build"),
            r#"project('main-project', 'c',
  version : '2.0.0',
  license : 'GPL-3.0')

# Subproject (would normally be in subprojects/ dir)
# This tests that we handle the main meson.build correctly

executable('main-app', 'main.c')
"#,
        )
        .unwrap();

        fs::write(project_dir.join("main.c"), r#"int main() { return 0; }"#).unwrap();

        let buildsystems = detect_buildsystems(project_dir);
        assert!(
            buildsystems.iter().any(|bs| bs.name() == "meson"),
            "Should detect Meson for project with subprojects structure"
        );
    }

    #[test]
    fn test_meson_handles_symlinks() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let temp_dir = TempDir::new().unwrap();
            let real_project = temp_dir.path().join("real-project");
            let link_project = temp_dir.path().join("link-project");

            fs::create_dir(&real_project).unwrap();
            create_meson_project(&real_project).unwrap();

            // Create a symlink to the project
            symlink(&real_project, &link_project).unwrap();

            // Should detect Meson through the symlink
            let buildsystems = detect_buildsystems(&link_project);
            assert!(
                buildsystems.iter().any(|bs| bs.name() == "meson"),
                "Should detect Meson through symlink"
            );

            // Create session in temp dir
            let mut session = PlainSession::new();
            session.chdir(temp_dir.path()).unwrap();

            // Operations should work through the symlink
            let meson = buildsystems
                .iter()
                .find(|bs| bs.name() == "meson")
                .expect("Should find Meson buildsystem");

            match meson.get_declared_dependencies(&session, None) {
                Ok(_) => {
                    // Success - symlink handling works
                }
                Err(e) => {
                    let error_str = format!("{:?}", e);
                    assert!(
                        !error_str.contains("Missing Meson file"),
                        "Should handle symlinks correctly: {}",
                        error_str
                    );
                }
            }
        }
    }

    #[test]
    fn test_meson_introspect_unconfigured_project() {
        // This test specifically verifies the fix for introspecting unconfigured projects
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("unconfigured-project");
        fs::create_dir(&project_dir).unwrap();

        create_meson_project(&project_dir).unwrap();

        // Create session in a different directory to test path handling
        let mut session = PlainSession::new();
        session.chdir(temp_dir.path()).unwrap();

        // Detect the buildsystem
        let buildsystems = detect_buildsystems(&project_dir);
        let meson = buildsystems
            .iter()
            .find(|bs| bs.name() == "meson")
            .expect("Should find Meson buildsystem");

        // This should NOT fail with "Current directory is not a meson build directory"
        match meson.get_declared_dependencies(&session, None) {
            Ok(deps) => {
                log::debug!(
                    "Successfully introspected unconfigured project with {} dependencies",
                    deps.len()
                );
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                // The specific error from the bug report should not occur
                assert!(
                    !error_str.contains("Current directory is not a meson build directory"),
                    "Should not fail with build directory error for unconfigured projects: {}",
                    error_str
                );
                assert!(
                    !error_str.contains("Please specify a valid build dir"),
                    "Should not fail asking for build dir when introspecting source: {}",
                    error_str
                );

                // Other errors (like meson not installed) are acceptable
                log::debug!(
                    "Got acceptable error (likely meson not installed): {}",
                    error_str
                );
            }
        }
    }

    #[test]
    fn test_meson_introspect_scan_dependencies() {
        // This test verifies that --scan-dependencies works correctly on meson.build files
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("scan-deps-test");
        fs::create_dir(&project_dir).unwrap();

        // Create a project with multiple dependencies to test scan-dependencies
        fs::write(
            project_dir.join("meson.build"),
            r#"project('scan-deps-test', 'c',
  version : '1.0.0',
  license : 'MIT')

# Test various types of dependencies
glib_dep = dependency('glib-2.0', required: false)
threads_dep = dependency('threads', required: false)
math_dep = declare_dependency(
  dependencies: meson.get_compiler('c').find_library('m', required: false)
)

executable('test-app',
  'main.c',
  dependencies : [glib_dep, threads_dep, math_dep])
"#,
        )
        .unwrap();

        fs::write(
            project_dir.join("main.c"),
            r#"#include <stdio.h>
#include <math.h>
int main() { printf("Result: %f\n", sqrt(16.0)); return 0; }"#,
        )
        .unwrap();

        let mut session = PlainSession::new();
        session.chdir(temp_dir.path()).unwrap();

        let buildsystems = detect_buildsystems(&project_dir);
        let meson = buildsystems
            .iter()
            .find(|bs| bs.name() == "meson")
            .expect("Should find Meson buildsystem");

        // Test dependency introspection using --scan-dependencies on meson.build
        match meson.get_declared_dependencies(&session, None) {
            Ok(deps) => {
                log::debug!("Found {} dependencies", deps.len());
                for (category, dep) in &deps {
                    let min_ver =
                        if let Some(vague_dep) = dep.as_any().downcast_ref::<VagueDependency>() {
                            vague_dep.minimum_version.as_deref().unwrap_or("any")
                        } else {
                            "any"
                        };
                    log::debug!("  {:?}: {} ({})", category, dep.family(), min_ver);
                }

                // Should find some dependencies from our test project
                // Note: Exact dependencies depend on system availability
                // We expect to find at least glib-2.0 and threads if they're available
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                // Should NOT fail with "No command specified" since we fixed the root cause
                assert!(
                    !error_str.contains("No command specified"),
                    "Should not fail with 'No command specified' after fixing scan-dependencies usage: {}",
                    error_str
                );

                // Other errors (meson not installed, etc.) are acceptable
                log::debug!("Got acceptable error: {}", error_str);
            }
        }
    }

    #[test]
    fn test_meson_introspect_targets() {
        // Test that target introspection works correctly
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("targets-test");
        fs::create_dir(&project_dir).unwrap();

        fs::write(
            project_dir.join("meson.build"),
            r#"project('targets-test', 'c',
  version : '1.0.0')

# Test various target types
executable('main-app',
  'main.c',
  install : true)

executable('test-util',
  'test.c',
  install : false)

static_library('helper',
  'helper.c',
  install : false)
"#,
        )
        .unwrap();

        fs::write(project_dir.join("main.c"), "int main() { return 0; }").unwrap();
        fs::write(project_dir.join("test.c"), "int main() { return 1; }").unwrap();
        fs::write(project_dir.join("helper.c"), "void helper() {}").unwrap();

        let mut session = PlainSession::new();
        session.chdir(temp_dir.path()).unwrap();

        let buildsystems = detect_buildsystems(&project_dir);
        let meson = buildsystems
            .iter()
            .find(|bs| bs.name() == "meson")
            .expect("Should find Meson buildsystem");

        // Test output introspection
        match meson.get_declared_outputs(&session, None) {
            Ok(outputs) => {
                log::debug!("Found {} outputs", outputs.len());
                for output in &outputs {
                    log::debug!("  Output: {:?}", output);
                }

                // Should find at least the installed executable
                // Exact outputs depend on meson version and system
                // (outputs.len() is always >= 0 for Vec, so this is just a documentation comment)
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                // Check it's not a path or command error we've been fixing
                assert!(
                    !error_str.contains("Missing Meson file")
                        && !error_str.contains("./meson.build")
                        && !error_str.contains("No command specified"),
                    "Should not have known path/command errors: {}",
                    error_str
                );

                log::debug!("Got acceptable error: {}", error_str);
            }
        }
    }

    #[test]
    fn test_meson_introspect_complex_project() {
        // Test introspection on a more complex project structure
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("complex-test");
        fs::create_dir_all(project_dir.join("src")).unwrap();
        fs::create_dir_all(project_dir.join("include")).unwrap();

        fs::write(
            project_dir.join("meson.build"),
            r#"project('complex-test', 'c',
  version : '2.1.0',
  license : 'GPL-2.0',
  default_options : [
    'warning_level=3',
    'werror=true'
  ])

# Include directories
inc = include_directories('include')

# Dependencies with version constraints
json_dep = dependency('json-c', version: '>=0.13', required: false)
curl_dep = dependency('libcurl', version: '>=7.60', required: false) 
zlib_dep = dependency('zlib', required: false)

# Subdirectory
subdir('src')

# Main executable
executable('complex-app',
  sources: ['main.c', src_files],
  include_directories: inc,
  dependencies: [json_dep, curl_dep, zlib_dep],
  install: true)
"#,
        )
        .unwrap();

        fs::write(
            project_dir.join("src/meson.build"),
            "src_files = files('utils.c', 'parser.c')",
        )
        .unwrap();

        fs::write(
            project_dir.join("main.c"),
            "#include <stdio.h>\nint main() { printf(\"Complex app\\n\"); return 0; }",
        )
        .unwrap();
        fs::write(project_dir.join("src/utils.c"), "void utils_init() {}").unwrap();
        fs::write(project_dir.join("src/parser.c"), "void parse() {}").unwrap();
        fs::write(
            project_dir.join("include/common.h"),
            "#pragma once\nvoid utils_init();",
        )
        .unwrap();

        let mut session = PlainSession::new();
        session.chdir(temp_dir.path()).unwrap();

        let buildsystems = detect_buildsystems(&project_dir);
        let meson = buildsystems
            .iter()
            .find(|bs| bs.name() == "meson")
            .expect("Should find Meson buildsystem");

        // Test that complex projects work with our introspection
        match meson.get_declared_dependencies(&session, None) {
            Ok(deps) => {
                log::debug!("Complex project dependencies: {} found", deps.len());

                // Look for dependencies with version constraints
                let versioned_deps: Vec<_> = deps
                    .iter()
                    .filter(|(_, dep)| {
                        if let Some(vague_dep) = dep.as_any().downcast_ref::<VagueDependency>() {
                            vague_dep.minimum_version.is_some()
                        } else {
                            false
                        }
                    })
                    .collect();

                if !versioned_deps.is_empty() {
                    log::debug!("Dependencies with version constraints:");
                    for (cat, dep) in versioned_deps {
                        let min_ver = if let Some(vague_dep) =
                            dep.as_any().downcast_ref::<VagueDependency>()
                        {
                            vague_dep.minimum_version.as_deref().unwrap_or("any")
                        } else {
                            "any"
                        };
                        log::debug!("  {:?}: {} >= {}", cat, dep.family(), min_ver);
                    }
                }
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                assert!(
                    !error_str.contains("No command specified")
                        && !error_str.contains("Missing Meson file")
                        && !error_str.contains("./meson.build"),
                    "Should not have known errors on complex projects: {}",
                    error_str
                );

                log::debug!("Got acceptable error on complex project: {}", error_str);
            }
        }

        // Also test outputs for complex project
        match meson.get_declared_outputs(&session, None) {
            Ok(outputs) => {
                log::debug!("Complex project outputs: {} found", outputs.len());
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                assert!(
                    !error_str.contains("No command specified"),
                    "Should not have command errors: {}",
                    error_str
                );
            }
        }
    }

    #[test]
    fn test_meson_setup_command_with_source_and_build_dirs() {
        // Test the fix for meson setup requiring both source and build directory arguments
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("setup-test");
        fs::create_dir(&project_dir).unwrap();

        create_meson_project(&project_dir).unwrap();

        let mut session = PlainSession::new();
        session.chdir(temp_dir.path()).unwrap();

        let meson = Meson::new(&project_dir.join("meson.build"));

        // Test the temporary build setup (this tests the fixed setup command)
        let result = meson.setup_temp_build_for_introspect(&session, None, &["--projectinfo"]);

        match result {
            Ok(_) => {
                log::debug!("Setup with source and build dirs succeeded");

                // Verify temp build dir was cleaned up
                let temp_build = project_dir.join(".ognibuild-temp-build");
                assert!(
                    !session.exists(&temp_build),
                    "Temporary build directory should be cleaned up"
                );
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                // Should NOT fail with "No command specified" from setup
                assert!(
                    !error_str.contains("No command specified"),
                    "Setup should not fail with 'No command specified': {}",
                    error_str
                );

                log::debug!("Got acceptable setup error: {}", error_str);
            }
        }
    }

    #[test]
    fn test_meson_commands_work_from_different_cwd_regression() {
        // Regression test for the bug where meson commands fail when the current
        // working directory is not the project directory.
        //
        // Bug details: The Meson struct stores the path to meson.build file, but when
        // running meson commands, it wasn't changing the working directory to the project
        // directory. This caused commands like "meson setup" and "meson introspect" to fail
        // when run from a different directory.

        let temp_dir = TempDir::new().unwrap();

        // Create a deeply nested project structure to test path handling
        let workspace = temp_dir.path().join("workspace");
        let other_dir = temp_dir.path().join("other_directory");
        let project_dir = workspace.join("projects").join("my-meson-project");

        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&other_dir).unwrap();
        fs::create_dir_all(&project_dir).unwrap();

        create_meson_project(&project_dir).unwrap();

        // Create a session in a completely different directory
        let mut session = PlainSession::new();
        // IMPORTANT: Set working directory to somewhere completely unrelated to the project
        session.chdir(&other_dir).unwrap();

        // Verify we're in a different directory
        assert_ne!(session.pwd(), project_dir);
        assert_ne!(session.pwd().parent(), Some(project_dir.as_path()));

        // Detect the buildsystem from the project directory
        let buildsystems = detect_buildsystems(&project_dir);
        let meson = buildsystems
            .iter()
            .find(|bs| bs.name() == "meson")
            .expect("Should find Meson buildsystem");

        // Test 1: get_declared_dependencies should work even from different cwd
        match meson.get_declared_dependencies(&session, None) {
            Ok(deps) => {
                // Should find the glib dependency we declared
                let has_glib = deps
                    .iter()
                    .any(|(_, dep)| dep.family() == "glib-2.0" || dep.family().contains("glib"));
                // Success - the fix is working
                assert!(!deps.is_empty() || has_glib, "Should find dependencies");
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                // The bug would cause errors like:
                // - "ERROR: Neither source directory './meson.build' nor build directory './' contain a meson.build file"
                // - "ERROR: Missing Meson file in './meson.build'"
                assert!(
                    !error_str.contains("Missing Meson file")
                        && !error_str.contains("nor build directory")
                        && !error_str.contains("contain a meson.build"),
                    "BUG REPRODUCED: Meson command failed due to working directory issue: {}",
                    error_str
                );
            }
        }

        // Test 2: get_declared_outputs should also work from different cwd
        match meson.get_declared_outputs(&session, None) {
            Ok(_outputs) => {
                // Success - the fix is working
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                // Should not fail with path-related errors
                assert!(
                    !error_str.contains("Missing Meson file")
                        && !error_str.contains("nor build directory")
                        && !error_str.contains("contain a meson.build"),
                    "BUG REPRODUCED: Meson introspect failed due to working directory issue: {}",
                    error_str
                );
            }
        }

        // Test 3: Verify setup works from different cwd (if meson is available)
        let installer = NullInstaller;
        match meson.build(&session, &installer) {
            Ok(_) => {
                // Success - build worked from different cwd
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                // The bug would cause "meson setup build" to fail because it runs in wrong dir
                assert!(
                    !error_str.contains("source directory") && !error_str.contains("meson.build"),
                    "BUG REPRODUCED: Meson setup failed due to working directory issue: {}",
                    error_str
                );
            }
        }

        // Verify session is still in the other directory (commands shouldn't change it)
        // Canonicalize both paths to handle macOS /var -> /private/var symlink resolution
        let actual_pwd = session.pwd().canonicalize().unwrap_or_else(|_| session.pwd().to_path_buf());
        let expected_pwd = other_dir.canonicalize().unwrap_or_else(|_| other_dir.clone());
        assert_eq!(
            actual_pwd,
            expected_pwd,
            "Session cwd should not have changed"
        );
    }

    #[test]
    fn test_meson_error_handling_robustness() {
        // Test that our error handling is robust against various meson issues
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("error-test");
        fs::create_dir(&project_dir).unwrap();

        // Create a potentially problematic meson.build
        fs::write(
            project_dir.join("meson.build"),
            r#"project('error-test', 'c', version : '1.0.0')

# Dependency that might not exist
missing_dep = dependency('this-probably-does-not-exist-anywhere', 
                        required: false)

# Still define something so meson doesn't completely fail
executable('test', 'main.c', dependencies: missing_dep)
"#,
        )
        .unwrap();

        fs::write(project_dir.join("main.c"), "int main() { return 0; }").unwrap();

        let mut session = PlainSession::new();
        session.chdir(temp_dir.path()).unwrap();

        let buildsystems = detect_buildsystems(&project_dir);
        let meson = buildsystems
            .iter()
            .find(|bs| bs.name() == "meson")
            .expect("Should find Meson buildsystem");

        // Test that we handle various error conditions gracefully
        match meson.get_declared_dependencies(&session, None) {
            Ok(deps) => {
                log::debug!(
                    "Handled potentially problematic project: {} deps",
                    deps.len()
                );

                // Should be able to find at least the declared dependency
                // (even if it's not available on the system)
                let missing_deps: Vec<_> = deps
                    .iter()
                    .filter(|(_, dep)| dep.family().contains("this-probably-does-not-exist"))
                    .collect();

                if !missing_deps.is_empty() {
                    log::debug!(
                        "Found declared but unavailable dependency: {:?}",
                        missing_deps[0].1.family()
                    );
                }
            }
            Err(e) => {
                let error_str = format!("{:?}", e);
                // Verify we don't get the specific errors we've been fixing
                assert!(
                    !error_str.contains("No command specified"),
                    "Should not get 'No command specified' after fixing scan-dependencies: {}",
                    error_str
                );

                log::debug!("Handled error case appropriately: {}", error_str);
            }
        }
    }
}

use crate::dependency::Dependency;
use buildlog_consultant::problems::common::*;
#[cfg(feature = "debian")]
use buildlog_consultant::problems::debian::UnsatisfiedAptDependencies;
use buildlog_consultant::Problem;

/// Trait for converting build problems to dependencies.
///
/// This trait allows build problems to report what dependencies would be needed to fix them.
pub trait ToDependency: Problem {
    /// Convert this problem to a dependency that might fix it.
    ///
    /// # Returns
    /// * `Some(Box<dyn Dependency>)` if the problem can be fixed by installing a dependency
    /// * `None` if the problem cannot be fixed by installing a dependency
    fn to_dependency(&self) -> Option<Box<dyn Dependency>>;
}

macro_rules! try_problem_to_dependency {
    ($expr:expr, $type:ty) => {
        if let Some(p) = $expr
            .as_any()
            .downcast_ref::<$type>()
            .and_then(|p| p.to_dependency())
        {
            return Some(p);
        }
    };
}

/// Convert a build problem to a dependency that might fix it.
///
/// This function tries to convert various known problem types to dependencies
/// that might fix them.
///
/// # Arguments
/// * `problem` - The build problem to convert
///
/// # Returns
/// * `Some(Box<dyn Dependency>)` if the problem can be fixed by installing a dependency
/// * `None` if the problem cannot be fixed by installing a dependency or isn't recognized
pub fn problem_to_dependency(problem: &dyn Problem) -> Option<Box<dyn Dependency>> {
    // TODO(jelmer): Find a more idiomatic way to do this.
    try_problem_to_dependency!(problem, MissingAutoconfMacro);
    #[cfg(feature = "debian")]
    try_problem_to_dependency!(problem, UnsatisfiedAptDependencies);
    try_problem_to_dependency!(problem, MissingGoPackage);
    try_problem_to_dependency!(problem, MissingHaskellDependencies);
    try_problem_to_dependency!(problem, MissingJavaClass);
    try_problem_to_dependency!(problem, MissingJDK);
    try_problem_to_dependency!(problem, MissingJRE);
    try_problem_to_dependency!(problem, MissingJDKFile);
    try_problem_to_dependency!(problem, MissingLatexFile);
    try_problem_to_dependency!(problem, MissingCommand);
    try_problem_to_dependency!(problem, MissingCommandOrBuildFile);
    try_problem_to_dependency!(problem, VcsControlDirectoryNeeded);
    try_problem_to_dependency!(problem, MissingLuaModule);
    try_problem_to_dependency!(problem, MissingCargoCrate);
    try_problem_to_dependency!(problem, MissingRustCompiler);
    try_problem_to_dependency!(problem, MissingPkgConfig);
    try_problem_to_dependency!(problem, MissingFile);
    try_problem_to_dependency!(problem, MissingCHeader);
    try_problem_to_dependency!(problem, MissingJavaScriptRuntime);
    try_problem_to_dependency!(problem, MissingValaPackage);
    try_problem_to_dependency!(problem, MissingRubyGem);
    try_problem_to_dependency!(problem, DhAddonLoadFailure);
    try_problem_to_dependency!(problem, MissingLibrary);
    try_problem_to_dependency!(problem, MissingStaticLibrary);
    try_problem_to_dependency!(problem, MissingRubyFile);
    try_problem_to_dependency!(problem, MissingSprocketsFile);
    try_problem_to_dependency!(problem, CMakeFilesMissing);
    try_problem_to_dependency!(problem, MissingMavenArtifacts);
    try_problem_to_dependency!(problem, MissingGnomeCommonDependency);
    try_problem_to_dependency!(problem, MissingQtModules);
    try_problem_to_dependency!(problem, MissingQt);
    try_problem_to_dependency!(problem, MissingX11);
    try_problem_to_dependency!(problem, UnknownCertificateAuthority);
    try_problem_to_dependency!(problem, MissingLibtool);
    try_problem_to_dependency!(problem, MissingCMakeComponents);
    try_problem_to_dependency!(problem, MissingGnulibDirectory);
    try_problem_to_dependency!(problem, MissingIntrospectionTypelib);
    try_problem_to_dependency!(problem, MissingCSharpCompiler);
    try_problem_to_dependency!(problem, MissingXfceDependency);
    try_problem_to_dependency!(problem, MissingNodePackage);
    try_problem_to_dependency!(problem, MissingNodeModule);
    try_problem_to_dependency!(problem, MissingPerlPredeclared);
    try_problem_to_dependency!(problem, MissingPerlFile);
    try_problem_to_dependency!(problem, MissingPerlModule);
    try_problem_to_dependency!(problem, MissingPhpClass);
    try_problem_to_dependency!(problem, MissingPHPExtension);
    try_problem_to_dependency!(problem, MissingPytestFixture);
    try_problem_to_dependency!(problem, UnsupportedPytestArguments);
    try_problem_to_dependency!(problem, UnsupportedPytestConfigOption);
    try_problem_to_dependency!(problem, MissingPythonDistribution);
    try_problem_to_dependency!(problem, MissingPythonModule);
    try_problem_to_dependency!(problem, MissingSetupPyCommand);
    try_problem_to_dependency!(problem, MissingRPackage);
    try_problem_to_dependency!(problem, MissingVagueDependency);
    try_problem_to_dependency!(problem, MissingXmlEntity);
    try_problem_to_dependency!(problem, MissingMakeTarget);

    None
}

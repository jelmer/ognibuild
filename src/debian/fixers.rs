use crate::debian::apt::AptManager;
use crate::debian::context::{DebianPackagingContext, Error};
use crate::debian::fix_build::DebianBuildFixer;
use crate::dependencies::debian::{DebianDependency, TieBreaker};
use crate::session::Session;
use breezyshim::tree::Tree;
use buildlog_consultant::problems::common::NeedPgBuildExtUpdateControl;
use buildlog_consultant::sbuild::Phase;
use buildlog_consultant::Problem;
use debian_analyzer::editor::Editor;
use std::path::Path;

fn targeted_python_versions(tree: &dyn Tree, subpath: &Path) -> Vec<String> {
    let f = tree.get_file(&subpath.join("debian/control")).unwrap();
    let control = debian_control::Control::read(f).unwrap();
    let source = control.source().unwrap();
    let all = if let Some(build_depends) = source.build_depends() {
        build_depends
    } else {
        return vec![];
    };

    let targeted = vec![];
    for entry in all.entries() {
        for relation in entry.relations() {
            let mut targeted = vec![];
            if relation.name().starts_with("python3-") {
                targeted.push("python3".to_owned());
            }
            if relation.name().starts_with("pypy") {
                targeted.push("pypy".to_owned());
            }
            if relation.name().starts_with("python-") {
                targeted.push("python".to_owned());
            }
        }
    }
    targeted
}

pub struct PythonTieBreaker {
    targeted: Vec<String>,
}

impl PythonTieBreaker {
    fn from_tree(tree: &dyn Tree, subpath: &Path) -> Self {
        let targeted = targeted_python_versions(tree, subpath);
        Self { targeted }
    }
}

impl TieBreaker for PythonTieBreaker {
    fn break_tie<'a>(&self, reqs: &[&'a DebianDependency]) -> Option<&'a DebianDependency> {
        if self.targeted.is_empty() {
            return None;
        }

        fn same(pkg: &str, python_version: &str) -> bool {
            if pkg.starts_with(&format!("{}-", python_version)) {
                return true;
            }
            if pkg.starts_with(&format!("lib{}-", python_version)) {
                return true;
            }
            pkg == format!("lib{}-dev", python_version)
        }

        for python_version in &self.targeted {
            for req in reqs {
                if req
                    .package_names()
                    .iter()
                    .any(|name| same(name, &python_version))
                {
                    log::info!(
                        "Breaking tie between {:?} to {:?}, since package already has {} build-dependencies",
                        reqs,
                        req,
                        python_version,
                    );
                    return Some(req);
                }
            }
        }

        None
    }
}

fn retry_apt_failure(
    _error: &dyn Problem,
    _phase: &Phase,
    _context: &DebianPackagingContext,
) -> Result<bool, Error> {
    Ok(true)
}

fn enable_dh_autoreconf(context: &DebianPackagingContext, phase: &Phase) -> Result<bool, Error> {
    // Debhelper >= 10 depends on dh-autoreconf and enables autoreconf by default.
    let debhelper_compat_version =
        debian_analyzer::debhelper::get_debhelper_compat_level(&context.abspath(Path::new(".")))
            .unwrap();

    if !debhelper_compat_version
        .map(|dcv| dcv < 10)
        .unwrap_or(false)
    {
        return Ok(false);
    }

    let mut modified = false;

    let rules = context.edit_rules()?;
    for rule in rules.rules_by_target("%") {
        for (i, line) in rule.recipes().enumerate() {
            if !line.starts_with("dh ") {
                continue;
            }
            let new_line = debian_analyzer::rules::dh_invoke_add_with(&line, "autoreconf");
            if line != new_line {
                rule.replace_command(i, &new_line);
                modified = true;
            }
        }
    }

    if modified {
        context.add_dependency(phase, &DebianDependency::simple("dh-autoreconf"))
    } else {
        Ok(false)
    }
}

fn fix_missing_configure(
    _error: &dyn Problem,
    phase: &Phase,
    context: &DebianPackagingContext,
) -> Result<bool, Error> {
    if !context.has_filename(Path::new("configure.ac"))
        && !context.has_filename(Path::new("configure.in"))
    {
        return Ok(false);
    }

    enable_dh_autoreconf(context, phase)
}

fn fix_missing_automake_input(
    _error: &dyn Problem,
    phase: &Phase,
    context: &DebianPackagingContext,
) -> Result<bool, Error> {
    // TODO(jelmer): If it's ./NEWS, ./AUTHORS or ./README that's missing, then
    // try to set 'export AUTOMAKE = automake --foreign' in debian/rules.
    // https://salsa.debian.org/jelmer/debian-janitor/issues/88
    enable_dh_autoreconf(context, phase)
}

fn fix_missing_config_status_input(
    _error: &dyn Problem,
    _phase: &Phase,
    context: &DebianPackagingContext,
) -> Result<bool, Error> {
    let autogen_path = "autogen.sh";
    if !context.has_filename(Path::new(autogen_path)) {
        return Ok(false);
    }

    let mut rules = context.edit_rules()?;

    let rule_exists = rules
        .rules_by_target("override_dh_autoreconf")
        .next()
        .is_some();
    if rule_exists {
        return Ok(false);
    }

    let rule = rules.add_rule("override_dh_autoreconf");
    rule.push_command("dh_autoreconf ./autogen.sh");

    rules.commit()?;

    context.commit("Run autogen.sh during build.", None)
}

pub struct PackageDependencyFixer<'a, 'b, 'c>
where
    'c: 'a,
{
    apt: &'a AptManager<'c>,
    context: &'b DebianPackagingContext,
    tie_breakers: Vec<Box<dyn TieBreaker>>,
}

impl<'a, 'b, 'c> std::fmt::Display for PackageDependencyFixer<'a, 'b, 'c> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PackageDependencyFixer")
    }
}

impl<'a, 'b, 'c> std::fmt::Debug for PackageDependencyFixer<'a, 'b, 'c> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PackageDependencyFixer")
    }
}

impl<'a, 'b, 'c> DebianBuildFixer for PackageDependencyFixer<'a, 'b, 'c> {
    fn can_fix(&self, problem: &dyn Problem) -> bool {
        crate::buildlog::problem_to_dependency(problem).is_some()
    }

    fn fix(
        &self,
        problem: &dyn Problem,
        phase: &Phase,
    ) -> Result<bool, crate::fix_build::InterimError<Error>> {
        let dep = crate::buildlog::problem_to_dependency(problem).unwrap();

        let deb_dep = crate::debian::apt::dependency_to_deb_dependency(
            &self.apt,
            dep.as_ref(),
            self.tie_breakers.as_slice(),
        )
        .unwrap();

        let deb_dep = if let Some(deb_dep) = deb_dep {
            deb_dep
        } else {
            return Ok(false);
        };

        Ok(self.context.add_dependency(phase, &deb_dep).unwrap())
    }
}

pub struct PgBuildExtOutOfDateControlFixer<'a, 'b, 'c, 'd>
where
    'a: 'c,
{
    session: &'a dyn Session,
    context: &'b DebianPackagingContext,
    apt: &'c AptManager<'d>,
}

impl<'a, 'b, 'c, 'd> std::fmt::Debug for PgBuildExtOutOfDateControlFixer<'a, 'b, 'c, 'd> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PgBuildExtOutOfDateControlFixer")
    }
}

impl<'a, 'b, 'c, 'd> std::fmt::Display for PgBuildExtOutOfDateControlFixer<'a, 'b, 'c, 'd> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PgBuildExtOutOfDateControlFixer")
    }
}

impl<'a, 'b, 'c, 'd> DebianBuildFixer for PgBuildExtOutOfDateControlFixer<'a, 'b, 'c, 'd> {
    fn can_fix(&self, problem: &dyn Problem) -> bool {
        problem
            .as_any()
            .downcast_ref::<NeedPgBuildExtUpdateControl>()
            .is_some()
    }

    fn fix(
        &self,
        error: &dyn Problem,
        _phase: &Phase,
    ) -> std::result::Result<bool, crate::fix_build::InterimError<crate::debian::context::Error>>
    {
        let error = error
            .as_any()
            .downcast_ref::<NeedPgBuildExtUpdateControl>()
            .unwrap();
        log::info!("Running 'pg_buildext updatecontrol'");
        self.apt
            .satisfy(vec![crate::debian::apt::SatisfyEntry::Required(
                "postgresql-common".to_string(),
            )])
            .unwrap();
        let project = self
            .session
            .project_from_directory(&self.context.tree.abspath(Path::new(".")).unwrap(), None)
            .unwrap();
        self.session
            .command(vec!["pg_buildext", "updatecontrol"])
            .cwd(&project.internal_path())
            .check_call()
            .unwrap();
        std::fs::copy(
            project.internal_path().join(&error.generated_path),
            self.context.abspath(Path::new(&error.generated_path)),
        )
        .unwrap();
        self.context
            .commit("Run 'pgbuildext updatecontrol'.", Some(false))?;
        Ok(true)
    }
}

fn fix_missing_makefile_pl(
    error: &buildlog_consultant::problems::common::MissingPerlFile,
    _phase: &Phase,
    context: &DebianPackagingContext,
) -> Result<bool, Error> {
    if error.filename == "Makefile.PL"
        && !context.has_filename(Path::new("Makefile.PL"))
        && context.has_filename(Path::new("dist.ini"))
    {
        // TODO(jelmer): add dist-zilla add-on to debhelper
        unimplemented!()
    }
    return Ok(false);
}

fn debcargo_coerce_unacceptable_prerelease(
    _error: &dyn Problem,
    _phase: &Phase,
    context: &DebianPackagingContext,
) -> Result<bool, Error> {
    let path = context.abspath(Path::new("debian/debcargo.toml"));
    let text = std::fs::read_to_string(&path)?;
    let mut doc: toml_edit::DocumentMut = text.parse().unwrap();
    doc.as_table_mut()["allow_prerelease_deps"] = toml_edit::value(true);
    std::fs::write(&path, doc.to_string())?;
    context.commit("Enable allow_prerelease_deps.", None)?;
    Ok(true)
}

macro_rules! simple_build_fixer {
    ($name:ident, $problem_cls:ty, $fn:expr) => {
        pub struct $name<'a>(&'a DebianPackagingContext);

        impl<'a> std::fmt::Display for $name<'a> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", stringify!($name))
            }
        }

        impl<'a> std::fmt::Debug for $name<'a> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", stringify!($name))
            }
        }

        impl<'a> DebianBuildFixer for $name<'a> {
            fn can_fix(&self, problem: &dyn Problem) -> bool {
                problem.as_any().downcast_ref::<$problem_cls>().is_some()
            }

            fn fix(
                &self,
                error: &dyn Problem,
                phase: &Phase,
            ) -> std::result::Result<
                bool,
                crate::fix_build::InterimError<crate::debian::context::Error>,
            > {
                let error = error.as_any().downcast_ref::<$problem_cls>().unwrap();
                $fn(error, phase, self.0).map_err(|e| crate::fix_build::InterimError::Other(e))
            }
        }
    };
}

simple_build_fixer!(
    MissingConfigureFixer,
    buildlog_consultant::problems::common::MissingConfigure,
    fix_missing_configure
);
simple_build_fixer!(
    MissingAutomakeInputFixer,
    buildlog_consultant::problems::common::MissingAutomakeInput,
    fix_missing_automake_input
);
simple_build_fixer!(
    MissingConfigStatusInputFixer,
    buildlog_consultant::problems::common::MissingConfigStatusInput,
    fix_missing_config_status_input
);
simple_build_fixer!(
    MissingPerlFileFixer,
    buildlog_consultant::problems::common::MissingPerlFile,
    fix_missing_makefile_pl
);
simple_build_fixer!(
    DebcargoUnacceptablePredicateFixer,
    buildlog_consultant::problems::debian::DebcargoUnacceptablePredicate,
    debcargo_coerce_unacceptable_prerelease
);
simple_build_fixer!(
    DebcargoUnacceptableComparatorFixer,
    buildlog_consultant::problems::debian::DebcargoUnacceptableComparator,
    debcargo_coerce_unacceptable_prerelease
);
simple_build_fixer!(
    RetryAptFetchFailure,
    buildlog_consultant::problems::debian::AptFetchFailure,
    retry_apt_failure
);

pub fn versioned_package_fixers<'a, 'b, 'c, 'd, 'e>(
    session: &'c dyn Session,
    packaging_context: &'b DebianPackagingContext,
    apt: &'a AptManager<'e>,
) -> Vec<Box<dyn DebianBuildFixer + 'd>>
where
    'a: 'd,
    'b: 'd,
    'c: 'd,
    'c: 'a,
{
    vec![
        Box::new(PgBuildExtOutOfDateControlFixer {
            context: packaging_context,
            session,
            apt,
        }),
        Box::new(MissingConfigureFixer(packaging_context)),
        Box::new(MissingAutomakeInputFixer(packaging_context)),
        Box::new(MissingConfigStatusInputFixer(packaging_context)),
        Box::new(MissingPerlFileFixer(packaging_context)),
        Box::new(DebcargoUnacceptablePredicateFixer(packaging_context)),
        Box::new(DebcargoUnacceptableComparatorFixer(packaging_context)),
    ]
}

pub fn apt_fixers<'a, 'b, 'c, 'd>(
    apt: &'a AptManager<'d>,
    packaging_context: &'b DebianPackagingContext,
) -> Vec<Box<dyn DebianBuildFixer + 'c>>
where
    'a: 'c,
    'b: 'c,
{
    let apt_tie_breakers: Vec<Box<dyn TieBreaker>> = vec![
        Box::new(PythonTieBreaker::from_tree(
            &packaging_context.tree,
            &packaging_context.subpath,
        )),
        Box::new(crate::debian::build_deps::BuildDependencyTieBreaker::from_session(apt.session())),
        #[cfg(feature = "udd")]
        Box::new(crate::debian::udd::PopconTieBreaker),
    ];
    vec![
        Box::new(RetryAptFetchFailure(packaging_context)) as Box<dyn DebianBuildFixer>,
        Box::new(PackageDependencyFixer {
            context: packaging_context,
            apt,
            tie_breakers: apt_tie_breakers,
        }) as Box<dyn DebianBuildFixer + 'c>,
    ]
}

pub fn default_fixers<'a, 'b, 'c, 'd>(
    packaging_context: &'a DebianPackagingContext,
    apt: &'b AptManager<'d>,
) -> Vec<Box<dyn DebianBuildFixer + 'c>>
where
    'a: 'c,
    'b: 'c,
{
    let mut ret = Vec::new();
    ret.extend(versioned_package_fixers(
        apt.session(),
        packaging_context,
        apt,
    ));
    ret.extend(apt_fixers(apt, packaging_context));
    ret
}

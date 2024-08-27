pub struct UpstreamInfo {
    name: Option<String>,
    buildsystem: Option<String>,
    branch_url: Option<String>,
    branch_subpath: Option<String>,
    tarball_url: Option<String>,
    metadata: Option<HashMap<String, String>>,
}

impl UpstreamInfo {
    pub fn version(&self) -> Option<String> {
        self.metadata.get("Version").map(|v| v.to_string())
    }
}

pub trait UpstreamFinder {
    fn find_upstream(&self) -> Option<UpstreamInfo>;
}

fn debian_go_base_name(package: &str) -> String {
    let (mut hostname, path) = package.split_once("/").unwrap();
    if hostname == "github.com" {
        hostname = "github";
    }
    if hostname == "gopkg.in" {
        hostname = "gopkg";
    }
    let path = path.rstrip("/").replace("/", "-");
    if let Some(rest) = path.strip_suffix(".git") {
        path = rest.to_string();
    }
    [hostname, path].replace("_", "-").to_lowercase()
}

pub fn find_python_package_upstream(dependency: &crate::dependencies::python::PythonDependency) -> Option<UpstreamInfo> {
    pypi_upstream_info(&dependency.package)
}


pub fn find_go_package_upstream(requirement: &crates::dependencies::go::GoPackageDependency) -> Option<UpstreamInfo> {
    if requirement.package.starts_with("github.com/") {
        let metadata = HashMap::new();
        metadata.insert("Go-Import-Path", requirement.package);
        Some(UpstreamInfo{
            name: format!("golang-{}", go_base_name(requirement.package)),
            metadata,
            branch_url: format!("https://{}", requirement.package.split("/")[..3].join("/")),
            branch_subpath: Some(""),
        })
    } else {
        None
    }
}

pub fn pypi_upstream_info(project: &str, version: Option<&str>) -> Result<Option<UpstreamInfo>, crate::ProviderError> {
    let pypi_data = load_pypi_project(project)?;
    let upstream_branch = pypi_data.info.project_urls.iter()
        .find(|(name, _)| name.to_lowercase() == "github" || name.to_lowercase() == "repository")
        .map(|(_, url)| url.to_string());

    let tarball_url = pypi_data.urls.iter()
        .find(|url_data| url_data.package_type == "sdist")
        .map(|url_data| url_data.url.to_string());

    Ok(Some(UpstreamInfo {
        branch_url: upstream_branch,
        branch_subpath: "",
        name: format!("python-{}", pypi_data.info.name),
        tarball_url,
    }))
}

pub fn find_perl_module_upstream(requirement: &crate::dependencies::perl::PerlModuleDependency) -> Option<UpstreamInfo> {
    perl_upstream_info(requirement.module)
}


fn cargo_upstream_info(cratename: &str, version: Option<&semver::Version>, api_version: Option<&semver::Version>) -> Option<UpstreamInfo> {
    let data = load_crate_info(cratename)?;
    // TODO(jelmer): Use upstream ontologist to parse upstream metadata
    let upstream_branch = data.crate_.repository;
    let name = format!("rust-{}",  data.crate_.name.replace("_", "-"));
    if version.is_some() {
    } else if let Some(api_version) = api_version {
        for version_info in data.versions {
            if !version_info.num.starts_with(&format!("{}.", api_version)) && version_info.num != api_version {
                continue;
            }
            if version.is_none() {
                version = Some(version_info.num);
            } else {
                version = semver::Version::max(version, version_info.num);
            }
        }
        if version.is_none() {
            log::warn!(
                "Unable to find version of crate {} that matches API version {}",
                name,
                api_version,
            );
        } else {
            name += format!("-{}.{}", version.major, version.minor);
        }
    }
    let metadata = HashMap::new();
    metadata.insert("Cargo-Crate", data.crate_.name);
    if let Some(version) = version {
        metadata.insert("Version", version.to_string());
    }

    return UpstreamInfo{
        branch_url:upstream_branch,
        branch_subpath:None,
        name,
        metadata,
        buildsystem:"cargo",
    }
}


fn find_cargo_crate_upstream(requirement: &crates::dependencies::CargoCrateDependency) -> Option<UpstreamInfo> {
    cargo_upstream_info(
        requirement.cratename, requirement.api_version
    )
}


fn apt_to_cargo_requirement(m, rels) {
    name = m.group(1)
    api_version = m.group(2)
    if m.group(3):
        features = set(m.group(3)[1:].split("-"))
    else:
        features = set()
    if not rels:
        minimum_version = None
    elif len(rels) == 1 and rels[0][0] == ">=":
        minimum_version = Version(rels[0][1]).upstream_version
    else:
        logging.warning("Unable to parse Debian version %r", rels)
        minimum_version = None

    return CargoCrateRequirement(
        name,
        api_version=api_version,
        features=features,
        minimum_version=minimum_version,
    )
}


fn apt_to_python_requirement(m, rels) {
    name = m.group(2)
    python_version = m.group(1)
    if not rels:
        minimum_version = None
    elif len(rels) == 1 and rels[0][0] == ">=":
        minimum_version = Version(rels[0][1]).upstream_version
    else:
        logging.warning("Unable to parse Debian version %r", rels)
        minimum_version = None
    return PythonPackageRequirement(
        name,
        python_version=(python_version or None),
        minimum_version=minimum_version,
    )
}

fn apt_to_ruby_requirement(m, rels) {
    if not rels:
        minimum_version = None
    elif len(rels) == 1 and rels[0][0] == ">=":
        minimum_version = Version(rels[0][1]).upstream_version
    else:
        logging.warning("Unable to parse Debian version %r", rels)
        minimum_version = None
    return RubyGemRequirement(m.group(1), minimum_version)
}


fn apt_to_go_requirement(m, rels) {
    parts = m.group(1).split("-")
    if parts[0] == "github":
        parts[0] = "github.com"
    if parts[0] == "gopkg":
        parts[0] = "gopkg.in"
    if not rels:
        version = None
    elif len(rels) == 1 and rels[0][0] == "=":
        version = Version(rels[0][1]).upstream_version
    else:
        logging.warning("Unable to parse Debian version %r", rels)
        version = None
    return GoPackageRequirement("/".join(parts), version=version)
}


BINARY_PACKAGE_UPSTREAM_MATCHERS = [
    (r"librust-(.*)-([^-+]+)(\+.*?)-dev", apt_to_cargo_requirement),
    (r"python([0-9.]*)-(.*)", apt_to_python_requirement),
    (r"golang-(.*)-dev", apt_to_go_requirement),
    (r"ruby-(.*)", apt_to_ruby_requirement),
]


_BINARY_PACKAGE_UPSTREAM_MATCHERS = [
    (re.compile(r), fn) for (r, fn) in BINARY_PACKAGE_UPSTREAM_MATCHERS
]


def find_apt_upstream(requirement: AptRequirement) -> Optional[UpstreamInfo]:
    for option in requirement.relations:
        for rel in option:
            for matcher, fn in _BINARY_PACKAGE_UPSTREAM_MATCHERS:
                m = matcher.fullmatch(rel["name"])
                if m:
                    upstream_requirement = fn(
                        m, [rel["version"]] if rel["version"] else []
                    )
                    return find_upstream(upstream_requirement)

            logging.warning(
                "Unable to map binary package name %s to upstream", rel["name"]
            )
    return None


def find_or_upstream(requirement: OneOfRequirement) -> Optional[UpstreamInfo]:
    for req in requirement.elements:
        info = find_upstream(req)
        if info is not None:
            return info
    return None



fn npm_upstream_info(package: &str, version: Option<&str>) -> Option<UpstreamInfo> {
    data = load_npm_package(package)
    if data is None:
        return None
    versions = data.get("versions", {})
    if version is not None:
        version_data = versions[version]
    else:
        version_data = versions[max(versions.keys())]
    if "repository" in version_data:
        try:
            branch_url = version_data["repository"]["url"]
        except (TypeError, KeyError):
            logging.warning(
                "Unexpectedly formatted repository data: %r",
                version_data["repository"],
            )
            branch_url = None
    else:
        branch_url = None
    return UpstreamInfo(
        branch_url=branch_url,
        branch_subpath="",
        name=f"node-{package}",
        tarball_url=version_data["dist"]["tarball"],
    )
}


pub fn find_npm_upstream(requirement: &crates::dependencies::node::NodePackageDependency) -> Option<UpstreamInfo> {
    npm_upstream_info(requirement.package)
}


pub fn load_cpan_module(module: &crate::dependencies::perl::PerlModuleDependency) -> Option<serde_json::Value> {
    upstream_ontologist::perl::load_cpan_module(module.name)
}


pub fn perl_upstream_info(module: &str, version: Option<&str>) -> Option<UpstreamInfo> {
    let data = load_cpan_module(module).unwrap();
    if data.is_none() {
        return None;
    }
    let release_metadata = data["release"]["_source"]["metadata"]
    release_resources = release_metadata.get("resources", {})
    branch_url = release_resources.get("repository", {}).get("url")
    metadata = {}
    metadata["Version"] = data["version"]
    return UpstreamInfo(
        name="lib{}-perl".format(module.lower().replace("::", "-")),
        metadata=metadata,
        branch_url=branch_url,
        branch_subpath="",
        tarball_url=data["download_url"],
    )


def haskell_upstream_info(package, version=None):
    data = load_hackage_package(package, version)
    if data is None:
        return None
    # TODO(jelmer): parse cabal file
    # upstream-ontologist has a parser..
    return UpstreamInfo(name=f"haskell-{package}")


def find_haskell_package_upstream(requirement):
    return haskell_upstream_info(requirement.package)


fn rubygem_upstream_info(gem: &str) -> Option<UpstreamInfo> {
    let data = load_rubygem(gem);
    if data.is_none() {
        return None;
    }
    metadata = {}
    homepage = data.get("homepage_uri")
    if homepage:
        metadata["Homepage"] = homepage
    bug_tracker = data.get("bug_tracker_uri")
    if bug_tracker:
        metadata["Bug-Database"] = bug_tracker
    metadata["Version"] = data["version"]
    return UpstreamInfo(
        name=f"ruby-{gem}",
        branch_url=data["source_code_uri"],
        metadata=metadata,
    )


fn find_rubygem_upstream(req: &crates::dependencies::ruby::RubyGemDependency) -> Option<UpstreamInfo> {
    rubygem_upstream_info(req.gem)
}


UPSTREAM_FINDER = {
    "python-package": find_python_package_upstream,
    "npm-package": find_npm_upstream,
    "go-package": find_go_package_upstream,
    "perl-module": find_perl_module_upstream,
    "cargo-crate": find_cargo_crate_upstream,
    "haskell-package": find_haskell_package_upstream,
    "apt": find_apt_upstream,
    "or": find_or_upstream,
    "gem": find_rubygem_upstream,
}


def find_upstream(requirement: Requirement) -> Optional[UpstreamInfo]:
    try:
        return UPSTREAM_FINDER[requirement.family](requirement)
    except KeyError:
        return None


pub fn find_upstream_from_repology(name: &str, version: Option<&str>) -> Option<UpstreamInfo> {
    let (family, name) = match name.split_once(":") {
        Some((family, name)) => (family, name),
        None => panic!("invalid repology name: {}", name),
    };
    match family {
        "python" => pypi_upstream_info(name, version),
        "go" => {
            let parts = name.split("-").collect::<Vec<_>>();
            if parts[0] == "github" {
                parts[0] = "github.com";
            }
            let metadata = HashMap::new();
            metadata.insert("Go-Import-Path", name);
            Some(UpstreamInfo{
                name: format!("golang-{}", go_base_name(name)),
                metadata,
                branch_url: format!("https://{}", parts[..3].join("/")),
                branch_subpath: "",
            })
        },
        "rust" => cargo_upstream_info(name, version),
        "node" => npm_upstream_info(name, version),
        "perl" => {
            let parts = name.split("-").collect::<Vec<_>>();
            fn capitalize(s: &str) -> String {
                s.chars().next().unwrap().to_uppercase().chain(s.chars().skip(1)).collect()
            }
            let module = parts.iter().map(|x| capitalize(x)).collect::<Vec<_>>().join("::");
            perl_upstream_info(module, version)
        }
        "haskell" => haskell_upstream_info(name, version),
    }
    // apmod, coq, cursors, deadbeef, emacs, erlang, fonts, fortunes, fusefs,
    // gimp, gstreamer, gtktheme, haskell, raku, ros, haxe, icons, java, js,
    // julia, ladspa, lisp, lua, lv2, mingw, nextcloud, nginx, nim, ocaml,
    // opencpn, rhythmbox texlive, tryton, vapoursynth, vdr, vim, xdrv,
    // xemacs
    return None
}

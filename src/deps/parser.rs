use crate::core::{DepInfo, Ecosystem, UpdateSeverity};
use chrono::Utc;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::debug;

#[derive(Debug, Clone)]
pub struct ManifestError {
    pub manifest: std::path::PathBuf,
    pub message: String,
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.manifest.display(), self.message)
    }
}

#[derive(Debug, Clone)]
pub struct ParseReport {
    pub dependencies: Vec<DepInfo>,
    pub errors: Vec<ManifestError>,
    pub non_public_skipped: bool,
}

type ParsedManifest = (Vec<DepInfo>, bool);
type ManifestParser = fn(&Path) -> ParsedManifest;

/// Detects and parses dependency manifests from a project directory.
pub struct DepsParser;

impl DepsParser {
    /// Auto-detect ecosystems present in the project and parse their dependencies.
    pub fn detect_and_parse(project_path: &Path) -> Vec<DepInfo> {
        Self::detect_and_report(project_path).dependencies
    }

    pub fn detect_and_report(project_path: &Path) -> ParseReport {
        let mut deps = Vec::new();
        let mut non_public_skipped = false;
        let parsers: [(&str, ManifestParser); 5] = [
            ("package.json", Self::parse_npm),
            ("Cargo.toml", Self::parse_cargo),
            ("go.mod", Self::parse_go),
            ("composer.json", Self::parse_composer),
            ("Gemfile", Self::parse_gem),
        ];
        for (manifest, parser) in parsers {
            if project_path.join(manifest).exists() {
                let (parsed, skipped) = parser(project_path);
                deps.extend(parsed);
                non_public_skipped |= skipped;
            }
        }
        if project_path.join("pyproject.toml").exists()
            || project_path.join("requirements.txt").exists()
        {
            let (parsed, skipped) = Self::parse_pip(project_path);
            deps.extend(parsed);
            non_public_skipped |= skipped;
        }

        let mut errors = Vec::new();
        for (manifest, format) in [
            ("package.json", ManifestSyntax::Json),
            ("Cargo.toml", ManifestSyntax::Toml),
            ("go.mod", ManifestSyntax::Text),
            ("pyproject.toml", ManifestSyntax::Toml),
            ("requirements.txt", ManifestSyntax::Text),
            ("composer.json", ManifestSyntax::Json),
            ("Gemfile", ManifestSyntax::Text),
        ] {
            let path = project_path.join(manifest);
            if !path.exists() {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let invalid = match format {
                        ManifestSyntax::Json => serde_json::from_str::<serde_json::Value>(&content)
                            .err()
                            .map(|error| error.to_string()),
                        ManifestSyntax::Toml => content
                            .parse::<toml::Value>()
                            .err()
                            .map(|error| error.to_string()),
                        ManifestSyntax::Text => None,
                    };
                    if let Some(message) = invalid {
                        errors.push(ManifestError {
                            manifest: path,
                            message,
                        });
                    }
                }
                Err(error) => errors.push(ManifestError {
                    manifest: path,
                    message: error.to_string(),
                }),
            }
        }
        ParseReport {
            dependencies: deps,
            errors,
            non_public_skipped,
        }
    }

    fn parse_npm(path: &Path) -> (Vec<DepInfo>, bool) {
        let manifest_path = path.join("package.json");
        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read package.json: {}", e);
                return (Vec::new(), false);
            }
        };

        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                debug!("Failed to parse package.json: {}", e);
                return (Vec::new(), false);
            }
        };
        let global_npmrc = dirs::home_dir()
            .map(|home| home.join(".npmrc"))
            .map(|path| match std::fs::read_to_string(&path) {
                Ok(content) => Ok(content),
                Err(_) if path.exists() => Err(()),
                Err(_) => Ok(String::new()),
            })
            .unwrap_or_else(|| Ok(String::new()));
        let global_npmrc = match global_npmrc {
            Ok(content) => content,
            Err(()) => return (Vec::new(), true),
        };
        let mut npmrc = if let Some(configuration) = std::env::var_os("NPM_CONFIG_USERCONFIG") {
            let path = PathBuf::from(configuration);
            match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(_) if path.exists() => return (Vec::new(), true),
                Err(_) => global_npmrc,
            }
        } else {
            global_npmrc
        };
        let yarnrc = dirs::home_dir()
            .map(|home| home.join(".yarnrc.yml"))
            .map(|path| match std::fs::read_to_string(&path) {
                Ok(content) => Ok(content),
                Err(_) if path.exists() => Err(()),
                Err(_) => Ok(String::new()),
            })
            .unwrap_or_else(|| Ok(String::new()));
        let mut yarnrc = match yarnrc {
            Ok(content) => content,
            Err(()) => return (Vec::new(), true),
        };
        for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            let npmrc_path = ancestor.join(".npmrc");
            match std::fs::read_to_string(&npmrc_path) {
                Ok(content) => {
                    npmrc.push('\n');
                    npmrc.push_str(&content);
                }
                Err(_) if npmrc_path.exists() => return (Vec::new(), true),
                Err(_) => {}
            }
            let yarnrc_path = ancestor.join(".yarnrc.yml");
            match std::fs::read_to_string(&yarnrc_path) {
                Ok(content) => {
                    yarnrc.push('\n');
                    yarnrc.push_str(&content);
                }
                Err(_) if yarnrc_path.exists() => return (Vec::new(), true),
                Err(_) => {}
            }
        }
        if let Ok(registry) =
            std::env::var("NPM_CONFIG_REGISTRY").or_else(|_| std::env::var("npm_config_registry"))
        {
            npmrc.push_str(&format!("\nregistry={registry}"));
        }
        if let Ok(registry) = std::env::var("YARN_NPM_REGISTRY_SERVER") {
            yarnrc.push_str(&format!("\nnpmRegistryServer: {registry}"));
        }
        if has_non_public_npm_default_registry(&npmrc) || has_non_public_yarn_registry(&yarnrc) {
            return (Vec::new(), true);
        }

        let mut deps = Vec::new();
        let mut non_public_skipped = false;
        let checked_at = Utc::now();

        // Parse "dependencies" and "devDependencies"
        for section in &["dependencies", "devDependencies"] {
            if let Some(obj) = json.get(section).and_then(|v| v.as_object()) {
                for (name, version_val) in obj {
                    if has_non_public_npm_scope_registry(&npmrc, name) {
                        non_public_skipped = true;
                        continue;
                    }
                    let version_str = version_val.as_str().unwrap_or("*");
                    if is_non_public_npm_spec(version_str) {
                        non_public_skipped = true;
                        continue;
                    }
                    // Strip semver range prefixes for current version
                    let clean_version = version_str
                        .trim_start_matches('^')
                        .trim_start_matches('~')
                        .trim_start_matches('>')
                        .trim_start_matches('=')
                        .to_string();

                    deps.push(DepInfo {
                        name: name.clone(),
                        ecosystem: Ecosystem::Npm,
                        current_version: clean_version,
                        latest_version: None, // Filled by registry client
                        update_available: false,
                        severity: UpdateSeverity::Info,
                        dev_dependency: *section == "devDependencies",
                        manifest_name: None,
                        checked_at,
                    });
                }
            }
        }

        (deps, non_public_skipped)
    }

    fn parse_cargo(path: &Path) -> (Vec<DepInfo>, bool) {
        if std::env::var("CARGO_REGISTRY_DEFAULT")
            .map(|registry| !registry.is_empty() && registry != "crates-io")
            .unwrap_or(false)
            || std::env::var_os("CARGO_SOURCE_CRATES_IO_REPLACE_WITH").is_some()
        {
            return (Vec::new(), true);
        }
        let cargo_home = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".cargo")));
        let configuration_paths = path
            .ancestors()
            .flat_map(|ancestor| {
                [
                    ancestor.join(".cargo/config.toml"),
                    ancestor.join(".cargo/config"),
                ]
            })
            .chain(
                cargo_home
                    .into_iter()
                    .flat_map(|home| [home.join("config.toml"), home.join("config")]),
            );
        for configuration_path in configuration_paths {
            if !configuration_path.exists() {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(configuration_path) else {
                return (Vec::new(), true);
            };
            let Ok(document) = content.parse::<toml::Value>() else {
                return (Vec::new(), true);
            };
            if document
                .get("source")
                .and_then(|source| source.get("crates-io"))
                .and_then(|crates| crates.get("replace-with"))
                .is_some()
                || document
                    .get("registry")
                    .map(|registry| {
                        !registry.is_table()
                            || registry
                                .get("default")
                                .is_some_and(|default| default.as_str() != Some("crates-io"))
                    })
                    .unwrap_or(false)
            {
                return (Vec::new(), true);
            }
        }
        let manifest_path = path.join("Cargo.toml");
        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read Cargo.toml: {}", e);
                return (Vec::new(), false);
            }
        };

        let toml_val: toml::Value = match toml::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                debug!("Failed to parse Cargo.toml: {}", e);
                return (Vec::new(), false);
            }
        };

        let mut deps = Vec::new();
        let mut non_public_skipped = false;
        let checked_at = Utc::now();
        let mut replaced = HashSet::new();
        if let Some(patch) = toml_val.get("patch").and_then(|value| value.as_table()) {
            for (_, dependencies) in patch {
                if let Some(dependencies) = dependencies.as_table() {
                    for (name, value) in dependencies {
                        replaced.insert(cargo_crate_name(name));
                        if let Some(package) = value.get("package").and_then(|value| value.as_str())
                        {
                            replaced.insert(cargo_crate_name(package));
                        }
                    }
                }
            }
        }
        if let Some(replace) = toml_val.get("replace").and_then(|value| value.as_table()) {
            for (name, value) in replace {
                replaced.insert(cargo_crate_name(&cargo_replacement_name(name)));
                if let Some(package) = value.get("package").and_then(|value| value.as_str()) {
                    replaced.insert(cargo_crate_name(package));
                }
            }
        }

        for section in &["dependencies", "dev-dependencies"] {
            if let Some(deps_table) = toml_val.get(section).and_then(|v| v.as_table()) {
                for (name, val) in deps_table {
                    let (registry_name, version) = match val {
                        toml::Value::String(s) => (name.clone(), s.clone()),
                        toml::Value::Table(t) => {
                            if ["path", "git", "workspace", "registry", "registry-index"]
                                .into_iter()
                                .any(|key| t.contains_key(key))
                            {
                                non_public_skipped = true;
                                continue;
                            }
                            let registry_name = t
                                .get("package")
                                .and_then(|value| value.as_str())
                                .unwrap_or(name)
                                .to_string();
                            let version = t
                                .get("version")
                                .and_then(|v| v.as_str())
                                .unwrap_or("*")
                                .to_string();
                            (registry_name, version)
                        }
                        _ => continue,
                    };

                    if replaced.contains(&cargo_crate_name(name))
                        || replaced.contains(&cargo_crate_name(&registry_name))
                    {
                        non_public_skipped = true;
                        continue;
                    }
                    let manifest_name = (registry_name != *name).then_some(name.clone());
                    deps.push(DepInfo {
                        name: registry_name,
                        ecosystem: Ecosystem::Cargo,
                        current_version: version,
                        latest_version: None,
                        update_available: false,
                        severity: UpdateSeverity::Info,
                        dev_dependency: *section == "dev-dependencies",
                        manifest_name,
                        checked_at,
                    });
                }
            }
        }

        (deps, non_public_skipped)
    }

    fn parse_go(path: &Path) -> (Vec<DepInfo>, bool) {
        if go_env_file_is_unreadable() {
            return (Vec::new(), true);
        }
        let workspace_path = match go_env_value("GOWORK").as_deref() {
            Some("off") => None,
            Some("") | Some("auto") | None => path
                .ancestors()
                .map(|ancestor| ancestor.join("go.work"))
                .find(|candidate| candidate.exists()),
            Some(workspace) => {
                let workspace = PathBuf::from(workspace);
                Some(if workspace.is_relative() {
                    path.join(workspace)
                } else {
                    workspace
                })
            }
        };
        if let Some(workspace_path) = workspace_path {
            let Ok(workspace) = std::fs::read_to_string(workspace_path) else {
                return (Vec::new(), true);
            };
            if workspace.lines().any(|line| {
                let line = line.trim();
                is_go_work_directive(line, "use") || is_go_work_directive(line, "replace")
            }) {
                return (Vec::new(), true);
            }
        }
        if go_proxy_is_non_public() {
            return (Vec::new(), true);
        }
        let mod_path = path.join("go.mod");
        let content = match std::fs::read_to_string(&mod_path) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read go.mod: {}", e);
                return (Vec::new(), false);
            }
        };

        let mut deps = Vec::new();
        let mut non_public_skipped = false;
        let checked_at = Utc::now();

        let mut local_replacements = HashSet::new();
        let mut in_replace_block = false;
        for line in content.lines() {
            let trimmed = line.split("//").next().unwrap_or("").trim();
            if is_go_block_start(trimmed, "replace") {
                in_replace_block = true;
                continue;
            }
            if trimmed == ")" {
                in_replace_block = false;
                continue;
            }
            if let Some(replacement) = trimmed
                .strip_prefix("replace")
                .filter(|rest| rest.starts_with(char::is_whitespace))
                .map(str::trim_start)
            {
                if let Some((module, _)) = replacement.split_once("=>") {
                    if let Some(module) = module.split_whitespace().next() {
                        local_replacements.insert(module.to_string());
                    }
                }
                continue;
            }
            if in_replace_block {
                if let Some((module, _)) = trimmed.split_once("=>") {
                    if let Some(module) = module.split_whitespace().next() {
                        local_replacements.insert(module.to_string());
                    }
                }
            }
        }

        // Simple line-based parsing of require blocks
        let mut in_require = false;
        for line in content.lines() {
            let trimmed = line.split("//").next().unwrap_or("").trim();
            if is_go_block_start(trimmed, "require") {
                in_require = true;
                continue;
            }
            if trimmed == ")" {
                in_require = false;
                continue;
            }
            if trimmed.starts_with("require ") && !trimmed.contains('(') {
                // Single-line require
                let parts: Vec<&str> = trimmed
                    .strip_prefix("require ")
                    .unwrap_or("")
                    .split_whitespace()
                    .collect();
                if parts.len() >= 2 && go_module_is_non_public(parts[0]) {
                    non_public_skipped = true;
                    continue;
                }
                if parts.len() >= 2 && local_replacements.contains(parts[0]) {
                    non_public_skipped = true;
                    continue;
                }
                if parts.len() >= 2 && !local_replacements.contains(parts[0]) {
                    deps.push(DepInfo {
                        name: parts[0].to_string(),
                        ecosystem: Ecosystem::Go,
                        current_version: parts[1].trim_start_matches('v').to_string(),
                        latest_version: None,
                        update_available: false,
                        severity: UpdateSeverity::Info,
                        dev_dependency: false,
                        manifest_name: None,
                        checked_at,
                    });
                }
                continue;
            }
            if in_require && !trimmed.is_empty() && !trimmed.starts_with("//") {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 && go_module_is_non_public(parts[0]) {
                    non_public_skipped = true;
                    continue;
                }
                if parts.len() >= 2 && local_replacements.contains(parts[0]) {
                    non_public_skipped = true;
                    continue;
                }
                if parts.len() >= 2 && !local_replacements.contains(parts[0]) {
                    deps.push(DepInfo {
                        name: parts[0].to_string(),
                        ecosystem: Ecosystem::Go,
                        current_version: parts[1].trim_start_matches('v').to_string(),
                        latest_version: None,
                        update_available: false,
                        severity: UpdateSeverity::Info,
                        dev_dependency: false,
                        manifest_name: None,
                        checked_at,
                    });
                }
            }
        }

        (deps, non_public_skipped)
    }

    fn parse_pip(path: &Path) -> (Vec<DepInfo>, bool) {
        let checked_at = Utc::now();
        let mut deps = Vec::new();
        let mut non_public_skipped = false;
        if has_non_public_python_environment(path) {
            return (Vec::new(), true);
        }

        // Try pyproject.toml first
        let uv_path = path.join("uv.toml");
        if uv_path.exists() {
            match std::fs::read_to_string(&uv_path)
                .ok()
                .and_then(|content| content.parse::<toml::Value>().ok())
            {
                Some(document) if !non_public_uv_configuration(&document) => {}
                _ => return (Vec::new(), true),
            }
        }
        let pyproject_path = path.join("pyproject.toml");
        let pyproject = std::fs::read_to_string(&pyproject_path)
            .ok()
            .and_then(|content| content.parse::<toml::Value>().ok());
        if pyproject_path.exists() && pyproject.is_none() {
            return (Vec::new(), true);
        }
        if let Some(toml_val) = &pyproject {
            if has_non_public_pyproject_indexes(toml_val) {
                return (Vec::new(), true);
            }
            // PEP 621 dependencies
            if let Some(deps_arr) = toml_val
                .get("project")
                .and_then(|p| p.get("dependencies"))
                .and_then(|d| d.as_array())
            {
                for dep in deps_arr {
                    if let Some(s) = dep.as_str() {
                        let (name, version) = Self::parse_pep508(s);
                        if name.is_empty() {
                            non_public_skipped = true;
                            continue;
                        }
                        if has_non_public_uv_source(toml_val, &name) {
                            non_public_skipped = true;
                            continue;
                        }
                        deps.push(DepInfo {
                            name,
                            ecosystem: Ecosystem::Pip,
                            current_version: version,
                            latest_version: None,
                            update_available: false,
                            severity: UpdateSeverity::Info,
                            dev_dependency: false,
                            manifest_name: None,
                            checked_at,
                        });
                    }
                }
            }
        }

        // Fallback to requirements.txt
        let req_path = path.join("requirements.txt");
        if req_path.exists() && deps.is_empty() {
            if let Ok(content) = std::fs::read_to_string(&req_path) {
                if has_non_public_pip_options(&content) {
                    return (Vec::new(), true);
                }
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
                        continue;
                    }
                    let (name, version) = Self::parse_requirement_line(trimmed);
                    if name.is_empty() {
                        non_public_skipped = true;
                        continue;
                    }
                    if pyproject
                        .as_ref()
                        .is_some_and(|document| pyproject_has_non_public_source(document, &name))
                    {
                        non_public_skipped = true;
                        continue;
                    }
                    deps.push(DepInfo {
                        name,
                        ecosystem: Ecosystem::Pip,
                        current_version: version,
                        latest_version: None,
                        update_available: false,
                        severity: UpdateSeverity::Info,
                        dev_dependency: false,
                        manifest_name: None,
                        checked_at,
                    });
                }
            }
        }

        (deps, non_public_skipped)
    }

    fn parse_composer(path: &Path) -> (Vec<DepInfo>, bool) {
        let global_configs = std::env::var_os("COMPOSER_HOME")
            .map(PathBuf::from)
            .into_iter()
            .chain(dirs::config_dir().map(|path| path.join("composer")))
            .chain(dirs::home_dir().map(|path| path.join(".composer")));
        for directory in global_configs {
            let path = directory.join("config.json");
            if !path.exists() {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(path) else {
                return (Vec::new(), true);
            };
            let Ok(document) = serde_json::from_str::<serde_json::Value>(&content) else {
                return (Vec::new(), true);
            };
            if has_non_public_composer_repositories(&document) {
                return (Vec::new(), true);
            }
        }
        let manifest_path = path.join("composer.json");
        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read composer.json: {}", e);
                return (Vec::new(), false);
            }
        };

        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                debug!("Failed to parse composer.json: {}", e);
                return (Vec::new(), false);
            }
        };

        if has_non_public_composer_repositories(&json) {
            return (Vec::new(), true);
        }

        let mut deps = Vec::new();
        let checked_at = Utc::now();

        for section in &["require", "require-dev"] {
            if let Some(obj) = json.get(section).and_then(|v| v.as_object()) {
                for (name, version_val) in obj {
                    if name == "php"
                        || name == "composer"
                        || name == "composer-plugin-api"
                        || name == "composer-runtime-api"
                        || name.starts_with("ext-")
                        || name.starts_with("lib-")
                        || name.starts_with("php-")
                        || !name.contains('/')
                    {
                        continue;
                    }
                    let version = version_val
                        .as_str()
                        .unwrap_or("*")
                        .trim_start_matches('^')
                        .trim_start_matches('~')
                        .to_string();

                    deps.push(DepInfo {
                        name: name.clone(),
                        ecosystem: Ecosystem::Composer,
                        current_version: version,
                        latest_version: None,
                        update_available: false,
                        severity: UpdateSeverity::Info,
                        dev_dependency: *section == "require-dev",
                        manifest_name: None,
                        checked_at,
                    });
                }
            }
        }

        (deps, false)
    }

    fn parse_gem(path: &Path) -> (Vec<DepInfo>, bool) {
        let bundle_configs = [path.join(".bundle/config")]
            .into_iter()
            .chain(dirs::home_dir().map(|home| home.join(".bundle/config")));
        for configuration in bundle_configs {
            if !configuration.exists() {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(configuration) else {
                return (Vec::new(), true);
            };
            if has_non_public_bundler_mirror(&content) {
                return (Vec::new(), true);
            }
        }
        if std::env::vars().any(|(name, value)| {
            name.starts_with("BUNDLE_MIRROR__")
                && !is_public_registry_url(&value, &["rubygems.org", "www.rubygems.org"])
        }) {
            return (Vec::new(), true);
        }
        let gemfile_path = path.join("Gemfile");
        let content = match std::fs::read_to_string(&gemfile_path) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read Gemfile: {}", e);
                return (Vec::new(), false);
            }
        };

        let mut deps = Vec::new();
        let mut non_public_skipped = false;
        let checked_at = Utc::now();
        let mut non_public_source = false;
        let mut block_stack: Vec<(Option<bool>, Option<bool>)> = Vec::new();

        for line in content.lines() {
            let trimmed = line.split('#').next().unwrap_or("").trim();
            if trimmed.starts_with("source") {
                let scoped_source = trimmed.ends_with(" do") || trimmed.contains(" do |");
                let non_public = gem_source_url(trimmed)
                    .map(|url| !is_public_registry_url(url, &["rubygems.org", "www.rubygems.org"]))
                    .unwrap_or(true);
                if scoped_source {
                    block_stack.push((Some(non_public), None));
                } else {
                    non_public_source |= non_public;
                }
                continue;
            }
            if trimmed.starts_with("group") {
                let development = trimmed.contains("development") || trimmed.contains("test");
                if trimmed.ends_with(" do") || trimmed.contains(" do |") {
                    block_stack.push((None, Some(development)));
                }
                continue;
            }
            if is_ruby_keyword_block(trimmed) {
                block_stack.push((None, None));
                continue;
            }
            if trimmed.ends_with(" do") || trimmed.contains(" do |") {
                let non_public = !["platforms", "platform", "install_if", "env"]
                    .iter()
                    .any(|allowed| trimmed.starts_with(allowed));
                block_stack.push((non_public.then_some(true), None));
                continue;
            }
            if trimmed == "end" {
                block_stack.pop();
                continue;
            }
            if block_stack
                .iter()
                .rev()
                .find_map(|(non_public, _)| *non_public)
                .unwrap_or(non_public_source)
            {
                non_public_skipped = true;
                continue;
            }
            if trimmed.is_empty() {
                continue;
            }
            let dev_group = block_stack
                .iter()
                .rev()
                .find_map(|(_, development)| *development)
                .unwrap_or(false)
                || inline_ruby_development_group(trimmed);
            // Parse: gem "name", "~> 1.0"
            if let Some(rest) = trimmed.strip_prefix("gem ") {
                if trimmed.ends_with(',') || trimmed.ends_with('\\') {
                    non_public_skipped = true;
                    continue;
                }
                if !gem_options_are_public(rest) {
                    non_public_skipped = true;
                    continue;
                }
                if ["path:", "git:", "github:", "bitbucket:", "source:"]
                    .into_iter()
                    .any(|option| rest.contains(option))
                    || rest.contains("=>")
                    || (trimmed.starts_with("path")
                        || trimmed.starts_with("git")
                        || trimmed.starts_with("source"))
                        && trimmed.ends_with(" do")
                {
                    non_public_skipped = true;
                    continue;
                }
                let parts = split_ruby_arguments(rest);
                if let Some(name_part) = parts.first() {
                    let name = name_part
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();
                    let compound = parts.len() > 2
                        && parts
                            .iter()
                            .skip(2)
                            .any(|part| part.chars().any(|character| character.is_ascii_digit()));
                    let version = if compound {
                        parts[1..].join(",").trim().to_string()
                    } else {
                        parts
                            .get(1)
                            .map(|v| {
                                let cleaned = v.trim().trim_matches('"').trim_matches('\'');
                                trim_gem_operator(cleaned)
                                    .split('#')
                                    .next()
                                    .unwrap_or("*")
                                    .trim()
                                    .to_string()
                            })
                            .unwrap_or_else(|| "*".to_string())
                    };

                    if !name.is_empty() {
                        deps.push(DepInfo {
                            name,
                            ecosystem: Ecosystem::Gem,
                            current_version: version,
                            latest_version: None,
                            update_available: false,
                            severity: UpdateSeverity::Info,
                            dev_dependency: dev_group,
                            manifest_name: None,
                            checked_at,
                        });
                    }
                }
            }
        }

        (deps, non_public_skipped)
    }

    /// Parse PEP 508 dependency specifier: "package>=1.0,<2.0" → ("package", "1.0")
    fn parse_pep508(spec: &str) -> (String, String) {
        let spec = spec.split('#').next().unwrap_or(spec).trim();
        if spec.is_empty()
            || spec.starts_with('.')
            || spec.starts_with('-')
            || spec.starts_with('/')
            || spec.starts_with('~')
            || spec.contains('/')
            || spec.contains("://")
            || spec.contains('@')
            || spec.ends_with(".whl")
            || spec.ends_with(".tar.gz")
            || spec.ends_with(".tgz")
            || spec.ends_with(".zip")
            || spec.ends_with(".egg")
        {
            return (String::new(), "*".to_string());
        }
        let split_pos = spec
            .find(['=', '>', '<', '!', '~', '[', ';', '@'])
            .unwrap_or(spec.len());
        let name = spec[..split_pos].trim().to_string();
        let has_extras = spec[split_pos..].starts_with('[');
        let version_source = if has_extras {
            spec[split_pos..]
                .find(']')
                .map(|end| &spec[split_pos + end + 1..])
                .unwrap_or("")
        } else {
            &spec[split_pos..]
        };
        let version_part = if has_extras || version_source.contains([',', ';']) {
            spec[split_pos..].trim().to_string()
        } else {
            version_source
                .trim()
                .trim_start_matches(">=")
                .trim_start_matches("~=")
                .trim_start_matches("==")
                .trim_start_matches('>')
                .trim_start_matches('=')
                .trim()
                .to_string()
        };
        (
            name,
            if version_part.is_empty() {
                "*".to_string()
            } else {
                version_part
            },
        )
    }

    /// Parse requirements.txt line: "package>=1.0" → ("package", "1.0")
    fn parse_requirement_line(line: &str) -> (String, String) {
        Self::parse_pep508(line)
    }
}

#[derive(Clone, Copy)]
enum ManifestSyntax {
    Json,
    Toml,
    Text,
}

fn split_ruby_arguments(declaration: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut square_depth = 0_usize;
    let mut paren_depth = 0_usize;
    for (index, character) in declaration.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => quote = Some(character),
            '[' => square_depth += 1,
            ']' => square_depth = square_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            ',' if square_depth == 0 && paren_depth == 0 => {
                parts.push(declaration[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(declaration[start..].trim());
    parts
}

fn inline_ruby_development_group(declaration: &str) -> bool {
    (declaration.contains("group:") || declaration.contains("groups:"))
        && (declaration.contains(":development") || declaration.contains(":test"))
}

fn trim_gem_operator(version: &str) -> &str {
    for operator in ["~>", ">=", "="] {
        if let Some(stripped) = version.strip_prefix(operator) {
            return stripped.trim_start();
        }
    }
    version
}

fn has_non_public_bundler_mirror(content: &str) -> bool {
    content.lines().any(|line| {
        if !line.to_ascii_lowercase().contains("mirror") {
            return false;
        }
        line.split_whitespace()
            .next_back()
            .map(|url| {
                !is_public_registry_url(
                    url.trim_matches(['"', '\'']),
                    &["rubygems.org", "www.rubygems.org"],
                )
            })
            .unwrap_or(false)
    })
}

fn gem_options_are_public(declaration: &str) -> bool {
    const SAFE_OPTIONS: [&str; 6] = [
        "require:",
        "group:",
        "groups:",
        "platforms:",
        "platform:",
        "install_if:",
    ];
    split_ruby_arguments(declaration)
        .into_iter()
        .skip(1)
        .all(|option| {
            let option = option.trim();
            !option.contains(':') || SAFE_OPTIONS.iter().any(|safe| option.starts_with(safe))
        })
}

fn gem_source_url(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("source")?.trim_start();
    let quote = rest
        .chars()
        .next()
        .filter(|character| matches!(character, '"' | '\''))?;
    let end = rest[1..].find(quote)?;
    Some(&rest[1..1 + end])
}

fn is_ruby_keyword_block(line: &str) -> bool {
    line == "begin"
        || [
            "if ", "unless ", "case ", "begin ", "def ", "class ", "module ", "while ", "until ",
            "for ",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
}

fn is_go_work_directive(line: &str, keyword: &str) -> bool {
    line == keyword
        || line.starts_with(&format!("{keyword} "))
        || line.starts_with(&format!("{keyword}("))
}

fn is_go_block_start(line: &str, keyword: &str) -> bool {
    line == format!("{keyword}(") || line.starts_with(&format!("{keyword} ("))
}

fn go_env_file_path() -> PathBuf {
    std::env::var_os("GOENV")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("~/.config"))
                .join("go")
                .join("env")
        })
}

fn go_env_file_is_unreadable() -> bool {
    let path = go_env_file_path();
    path.exists() && std::fs::read_to_string(path).is_err()
}

fn go_env_value(name: &str) -> Option<String> {
    if let Ok(value) = std::env::var(name) {
        return Some(value);
    }
    std::fs::read_to_string(go_env_file_path())
        .ok()?
        .lines()
        .find_map(|line| {
            line.split_once('=')
                .filter(|(key, _)| key.trim() == name)
                .map(|(_, value)| value.trim().to_string())
        })
}

fn go_proxy_is_non_public() -> bool {
    go_env_value("GOPROXY")
        .map(|proxy| {
            if proxy.is_empty() {
                return false;
            }
            let entries: Vec<&str> = proxy.split([',', '|']).map(str::trim).collect();
            let official = entries.first().is_some_and(|entry| {
                entry.eq_ignore_ascii_case("https://proxy.golang.org")
                    || entry.eq_ignore_ascii_case("https://proxy.golang.org/")
            });
            !official
                || entries.iter().skip(1).any(|entry| {
                    !entry.eq_ignore_ascii_case("direct") && !entry.eq_ignore_ascii_case("off")
                })
        })
        .unwrap_or(false)
}

fn go_module_is_non_public(module: &str) -> bool {
    ["GOPRIVATE", "GONOPROXY"].iter().any(|variable| {
        go_env_value(variable)
            .map(|patterns| {
                patterns
                    .split(',')
                    .any(|pattern| go_glob_match(pattern.trim(), module))
            })
            .unwrap_or(false)
    })
}

fn go_glob_match(pattern: &str, value: &str) -> bool {
    fn matches(pattern: &[char], value: &[char]) -> bool {
        match pattern.first() {
            None => value.is_empty(),
            Some('*') => {
                let remainder = &pattern[1..];
                matches(remainder, value) || (!value.is_empty() && matches(pattern, &value[1..]))
            }
            Some('?') => !value.is_empty() && matches(&pattern[1..], &value[1..]),
            Some(character) => {
                !value.is_empty() && value[0] == *character && matches(&pattern[1..], &value[1..])
            }
        }
    }
    if pattern.is_empty() {
        return false;
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    (0..=value.len()).any(|end| matches(&pattern, &value[..end]))
}

fn cargo_crate_name(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}

fn cargo_replacement_name(key: &str) -> String {
    key.rsplit('#')
        .next()
        .unwrap_or(key)
        .split(':')
        .next()
        .unwrap_or(key)
        .to_string()
}

pub(crate) fn pyproject_has_non_public_source(document: &toml::Value, package: &str) -> bool {
    if has_non_public_uv_source(document, package) {
        return true;
    }
    document
        .get("project")
        .and_then(|project| project.get("dependencies"))
        .and_then(|dependencies| dependencies.as_array())
        .map(|dependencies| {
            dependencies.iter().any(|dependency| {
                dependency.as_str().map(|specification| {
                    normalize_python_package_name(python_specification_name(specification))
                        == normalize_python_package_name(package)
                        && DepsParser::parse_pep508(specification).0.is_empty()
                }) == Some(true)
            })
        })
        .unwrap_or(false)
}

fn python_specification_name(specification: &str) -> &str {
    let end = specification
        .find(|character: char| {
            character == '='
                || character == '>'
                || character == '<'
                || character == '!'
                || character == '~'
                || character == '['
                || character == ';'
                || character == '@'
                || character.is_whitespace()
        })
        .unwrap_or(specification.len());
    specification[..end].trim()
}

fn has_non_public_uv_source(document: &toml::Value, package: &str) -> bool {
    document
        .get("tool")
        .and_then(|tool| tool.get("uv"))
        .and_then(|uv| uv.get("sources"))
        .and_then(|sources| sources.as_table())
        .and_then(|sources| {
            sources.iter().find_map(|(name, source)| {
                (normalize_python_package_name(name) == normalize_python_package_name(package))
                    .then_some(source)
            })
        })
        .map(non_public_uv_source_value)
        .unwrap_or(false)
}

fn non_public_uv_source_value(source: &toml::Value) -> bool {
    match source {
        toml::Value::Array(sources) => sources.iter().any(non_public_uv_source_value),
        toml::Value::Table(_) => {
            ["path", "git", "url"]
                .iter()
                .any(|key| source.get(*key).is_some())
                || source.get("workspace").and_then(|value| value.as_bool()) == Some(true)
                || source
                    .get("index")
                    .and_then(|value| value.as_str())
                    .map(|index| index != "pypi")
                    .unwrap_or(false)
        }
        _ => true,
    }
}

fn normalize_python_package_name(name: &str) -> String {
    let mut normalized = String::new();
    for character in name.chars() {
        if matches!(character, '-' | '_' | '.') {
            if !normalized.ends_with('-') {
                normalized.push('-');
            }
        } else {
            normalized.extend(character.to_lowercase());
        }
    }
    normalized
}

fn has_non_public_python_environment(path: &Path) -> bool {
    for variable in ["PIP_INDEX_URL", "UV_INDEX_URL", "UV_DEFAULT_INDEX"] {
        if std::env::var(variable)
            .map(|url| !is_public_registry_url(&url, &["pypi.org", "www.pypi.org"]))
            .unwrap_or(false)
        {
            return true;
        }
    }
    for variable in ["PIP_EXTRA_INDEX_URL", "UV_EXTRA_INDEX_URL"] {
        if std::env::var(variable)
            .map(|urls| {
                urls.split_whitespace()
                    .any(|url| !is_public_registry_url(url, &["pypi.org", "www.pypi.org"]))
            })
            .unwrap_or(false)
        {
            return true;
        }
    }
    for variable in [
        "PIP_NO_INDEX",
        "UV_NO_INDEX",
        "PIP_FIND_LINKS",
        "UV_FIND_LINKS",
    ] {
        if std::env::var_os(variable).is_some() {
            return true;
        }
    }
    let pip_configs = std::env::var_os("PIP_CONFIG_FILE")
        .map(PathBuf::from)
        .into_iter()
        .chain(dirs::config_dir().map(|path| path.join("pip/pip.conf")))
        .chain(dirs::home_dir().map(|path| path.join(".pip/pip.conf")))
        .chain([
            PathBuf::from("/etc/pip.conf"),
            PathBuf::from("/etc/xdg/pip/pip.conf"),
        ]);
    for path in pip_configs {
        if !path.exists() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            return true;
        };
        if has_non_public_pip_config(&content) {
            return true;
        }
    }
    let uv_configs = std::env::var_os("UV_CONFIG_FILE")
        .map(PathBuf::from)
        .into_iter()
        .chain(dirs::config_dir().map(|path| path.join("uv/uv.toml")))
        .chain([PathBuf::from("/etc/uv/uv.toml")])
        .chain(path.ancestors().map(|ancestor| ancestor.join("uv.toml")));
    for path in uv_configs {
        if !path.exists() {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            return true;
        };
        let Ok(document) = content.parse::<toml::Value>() else {
            return true;
        };
        if non_public_uv_configuration(&document) {
            return true;
        }
    }
    false
}

fn has_non_public_pip_config(content: &str) -> bool {
    let mut continuation = false;
    content.lines().any(|line| {
        if line.trim().is_empty() {
            return false;
        }
        if line.starts_with(char::is_whitespace) && continuation {
            return !is_public_registry_url(line.trim(), &["pypi.org", "www.pypi.org"]);
        }
        continuation = false;
        let line = line.split('#').next().unwrap_or("").trim();
        let Some(separator) = line.find(['=', ':']) else {
            return false;
        };
        let key = line[..separator].trim().to_ascii_lowercase();
        let value = line[separator + 1..].trim();
        match key.as_str() {
            "no-index" => matches!(
                value.to_ascii_lowercase().as_str(),
                "true" | "1" | "yes" | "on"
            ),
            "find-links" => true,
            "extra-index-url" => {
                continuation = true;
                !is_public_registry_url(value, &["pypi.org", "www.pypi.org"])
            }
            "index-url" | "default-index" => {
                !is_public_registry_url(value, &["pypi.org", "www.pypi.org"])
            }
            _ => false,
        }
    })
}

fn non_public_uv_configuration(uv: &toml::Value) -> bool {
    uv.get("no-index").and_then(|value| value.as_bool()) == Some(true)
        || uv.get("find-links").is_some()
        || uv
            .get("default-index")
            .map(non_public_pip_index_value)
            .unwrap_or(false)
        || ["index-url", "extra-index-url"].iter().any(|key| {
            uv.get(*key)
                .map(non_public_pip_index_value)
                .unwrap_or(false)
        })
        || non_public_pip_index_entries(uv.get("index"))
        || uv
            .get("pip")
            .map(non_public_uv_pip_settings)
            .unwrap_or(false)
}

fn has_non_public_pyproject_indexes(document: &toml::Value) -> bool {
    let Some(tool) = document.get("tool") else {
        return false;
    };
    if tool
        .get("uv")
        .map(non_public_uv_configuration)
        .unwrap_or(false)
    {
        return true;
    }
    ["poetry", "pdm"].iter().any(|manager| {
        tool.get(*manager)
            .and_then(|configuration| configuration.get("source"))
            .map(|source| match source {
                toml::Value::Array(entries) => entries.iter().any(|entry| {
                    entry
                        .get("url")
                        .map(non_public_pip_index_value)
                        .unwrap_or(true)
                }),
                _ => true,
            })
            .unwrap_or(false)
    })
}

fn non_public_uv_pip_settings(settings: &toml::Value) -> bool {
    settings.get("no-index").and_then(|value| value.as_bool()) == Some(true)
        || settings.get("find-links").is_some()
        || ["index-url", "extra-index-url"].iter().any(|key| {
            settings
                .get(*key)
                .map(non_public_pip_index_value)
                .unwrap_or(false)
        })
}

fn non_public_pip_index_entries(value: Option<&toml::Value>) -> bool {
    match value {
        Some(toml::Value::Array(entries)) => entries.iter().any(|entry| {
            entry
                .get("url")
                .map(non_public_pip_index_value)
                .unwrap_or(true)
        }),
        Some(value) => non_public_pip_index_value(value),
        None => false,
    }
}

fn non_public_pip_index_value(value: &toml::Value) -> bool {
    match value {
        toml::Value::String(url) => !is_public_registry_url(url, &["pypi.org", "www.pypi.org"]),
        toml::Value::Array(values) => values.iter().any(non_public_pip_index_value),
        toml::Value::Table(table) => table
            .get("url")
            .map(non_public_pip_index_value)
            .unwrap_or(true),
        _ => true,
    }
}

fn has_non_public_pip_options(content: &str) -> bool {
    content.lines().any(|line| {
        let line = line.split('#').next().unwrap_or("").trim();
        if !line.starts_with('-') {
            return false;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        tokens.iter().enumerate().any(|(index, token)| {
            if *token == "--no-index"
                || *token == "-f"
                || *token == "--find-links"
                || matches!(
                    *token,
                    "-r" | "--requirement" | "-c" | "--constraint" | "-e" | "--editable"
                )
                || token.starts_with("-f")
                || token.starts_with("-r")
                || token.starts_with("-c")
                || token.starts_with("-e")
                || token.starts_with("--find-links=")
                || token.starts_with("--requirement=")
                || token.starts_with("--constraint=")
                || token.starts_with("--editable=")
            {
                return true;
            }
            let url = if ["-i", "--index-url", "--extra-index-url"].contains(token) {
                Some(tokens.get(index + 1).copied().unwrap_or(""))
            } else if let Some(url) = token.strip_prefix("-i") {
                (!url.is_empty()).then_some(url)
            } else {
                ["--index-url=", "--extra-index-url="]
                    .iter()
                    .find_map(|prefix| token.strip_prefix(prefix))
            };
            url.map(|url| !is_public_registry_url(url, &["pypi.org", "www.pypi.org"]))
                .unwrap_or(false)
        })
    })
}

fn has_non_public_composer_repositories(json: &serde_json::Value) -> bool {
    let Some(repositories) = json.get("repositories") else {
        return false;
    };
    match repositories {
        serde_json::Value::Array(repositories) => {
            repositories.iter().any(is_non_public_composer_repository)
        }
        serde_json::Value::Object(repositories) => repositories.iter().any(|(name, repository)| {
            if name == "packagist.org" {
                if let Some(enabled) = repository.as_bool() {
                    return !enabled;
                }
            }
            is_non_public_composer_repository(repository)
        }),
        _ => true,
    }
}

fn is_non_public_composer_repository(repository: &serde_json::Value) -> bool {
    if !repository.is_object() || repository["packagist.org"].as_bool() == Some(false) {
        return true;
    }
    let repository_type = repository["type"].as_str().unwrap_or_default();
    let url = repository["url"].as_str().unwrap_or_default();
    repository_type != "composer"
        || !is_public_registry_url(url, &["repo.packagist.org", "packagist.org"])
}

fn is_public_registry_url(value: &str, public_hosts: &[&str]) -> bool {
    reqwest::Url::parse(value)
        .ok()
        .and_then(|url| url.host_str().map(|host| public_hosts.contains(&host)))
        .unwrap_or(false)
}

fn has_non_public_yarn_registry(yarnrc: &str) -> bool {
    yarnrc.lines().any(|line| {
        line.split_once(':')
            .filter(|(key, _)| key.trim() == "npmRegistryServer")
            .map(|(_, url)| {
                !is_public_registry_url(
                    url.trim().trim_matches(['"', '\'']),
                    &["registry.npmjs.org"],
                )
            })
            .unwrap_or(false)
    })
}

fn has_non_public_npm_default_registry(npmrc: &str) -> bool {
    npmrc
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with(['#', ';']) {
                return None;
            }
            line.split_once('=')
                .filter(|(key, _)| key.trim() == "registry")
                .map(|(_, url)| !is_public_registry_url(url.trim(), &["registry.npmjs.org"]))
        })
        .next_back()
        .unwrap_or(false)
}

fn has_non_public_npm_scope_registry(npmrc: &str, package: &str) -> bool {
    npmrc
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with(['#', ';']) {
                return None;
            }
            line.split_once('=').and_then(|(key, url)| {
                key.trim()
                    .strip_suffix(":registry")
                    .filter(|scope| scope.starts_with('@'))
                    .filter(|scope| package.starts_with(&format!("{scope}/")))
                    .map(|_| !is_public_registry_url(url.trim(), &["registry.npmjs.org"]))
            })
        })
        .next_back()
        .unwrap_or(false)
}

fn is_non_public_npm_spec(specification: &str) -> bool {
    let specification = specification.trim();
    [
        "file:",
        "link:",
        "workspace:",
        "git+",
        "http://",
        "https://",
        "git://",
        "github:",
        "gitlab:",
        "bitbucket:",
        "gist:",
        "portal:",
        "patch:",
        "npm:",
        "catalog:",
    ]
    .into_iter()
    .any(|prefix| specification.starts_with(prefix))
        || specification.starts_with('/')
        || specification.starts_with("./")
        || specification.starts_with("../")
        || specification.starts_with("~/")
        || specification.contains('/')
}

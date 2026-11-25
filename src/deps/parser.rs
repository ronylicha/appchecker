use crate::core::{AppError, DepInfo, Ecosystem, Result, UpdateSeverity};
use chrono::Utc;
use std::path::Path;
use tracing::debug;

/// Detects and parses dependency manifests from a project directory.
pub struct DepsParser;

impl DepsParser {
    /// Auto-detect ecosystems present in the project and parse their dependencies.
    pub fn detect_and_parse(project_path: &Path) -> Vec<DepInfo> {
        let mut deps = Vec::new();

        if project_path.join("package.json").exists() {
            deps.extend(Self::parse_npm(project_path));
        }
        if project_path.join("Cargo.toml").exists() {
            deps.extend(Self::parse_cargo(project_path));
        }
        if project_path.join("go.mod").exists() {
            deps.extend(Self::parse_go(project_path));
        }
        if project_path.join("pyproject.toml").exists()
            || project_path.join("requirements.txt").exists()
        {
            deps.extend(Self::parse_pip(project_path));
        }
        if project_path.join("composer.json").exists() {
            deps.extend(Self::parse_composer(project_path));
        }
        if project_path.join("Gemfile").exists() {
            deps.extend(Self::parse_gem(project_path));
        }

        deps
    }

    fn parse_npm(path: &Path) -> Vec<DepInfo> {
        let manifest_path = path.join("package.json");
        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read package.json: {}", e);
                return Vec::new();
            }
        };

        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                debug!("Failed to parse package.json: {}", e);
                return Vec::new();
            }
        };

        let mut deps = Vec::new();
        let checked_at = Utc::now();

        // Parse "dependencies" and "devDependencies"
        for section in &["dependencies", "devDependencies"] {
            if let Some(obj) = json.get(section).and_then(|v| v.as_object()) {
                for (name, version_val) in obj {
                    let version_str = version_val.as_str().unwrap_or("*");
                    // Strip semver range prefixes for current version
                    let clean_version = version_str
                        .trim_start_matches('^')
                        .trim_start_matches('~')
                        .trim_start_matches('>')
                        .trim_start_matches('=')
                        .trim_start_matches('<')
                        .to_string();

                    deps.push(DepInfo {
                        name: name.clone(),
                        ecosystem: Ecosystem::Npm,
                        current_version: clean_version,
                        latest_version: None, // Filled by registry client
                        update_available: false,
                        severity: UpdateSeverity::Info,
                        checked_at,
                    });
                }
            }
        }

        deps
    }

    fn parse_cargo(path: &Path) -> Vec<DepInfo> {
        let manifest_path = path.join("Cargo.toml");
        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read Cargo.toml: {}", e);
                return Vec::new();
            }
        };

        let toml_val: toml::Value = match toml::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                debug!("Failed to parse Cargo.toml: {}", e);
                return Vec::new();
            }
        };

        let mut deps = Vec::new();
        let checked_at = Utc::now();

        for section in &["dependencies", "dev-dependencies"] {
            if let Some(deps_table) = toml_val.get(section).and_then(|v| v.as_table()) {
                for (name, val) in deps_table {
                    let version = match val {
                        toml::Value::String(s) => s.clone(),
                        toml::Value::Table(t) => t
                            .get("version")
                            .and_then(|v| v.as_str())
                            .unwrap_or("*")
                            .to_string(),
                        _ => "*".to_string(),
                    };

                    deps.push(DepInfo {
                        name: name.clone(),
                        ecosystem: Ecosystem::Cargo,
                        current_version: version,
                        latest_version: None,
                        update_available: false,
                        severity: UpdateSeverity::Info,
                        checked_at,
                    });
                }
            }
        }

        deps
    }

    fn parse_go(path: &Path) -> Vec<DepInfo> {
        let mod_path = path.join("go.mod");
        let content = match std::fs::read_to_string(&mod_path) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read go.mod: {}", e);
                return Vec::new();
            }
        };

        let mut deps = Vec::new();
        let checked_at = Utc::now();

        // Simple line-based parsing of require blocks
        let mut in_require = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("require (") {
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
                if parts.len() >= 2 {
                    deps.push(DepInfo {
                        name: parts[0].to_string(),
                        ecosystem: Ecosystem::Go,
                        current_version: parts[1].trim_start_matches('v').to_string(),
                        latest_version: None,
                        update_available: false,
                        severity: UpdateSeverity::Info,
                        checked_at,
                    });
                }
                continue;
            }
            if in_require && !trimmed.is_empty() && !trimmed.starts_with("//") {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    deps.push(DepInfo {
                        name: parts[0].to_string(),
                        ecosystem: Ecosystem::Go,
                        current_version: parts[1].trim_start_matches('v').to_string(),
                        latest_version: None,
                        update_available: false,
                        severity: UpdateSeverity::Info,
                        checked_at,
                    });
                }
            }
        }

        deps
    }

    fn parse_pip(path: &Path) -> Vec<DepInfo> {
        let checked_at = Utc::now();
        let mut deps = Vec::new();

        // Try pyproject.toml first
        let pyproject_path = path.join("pyproject.toml");
        if pyproject_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&pyproject_path) {
                if let Ok(toml_val) = content.parse::<toml::Value>() {
                    // PEP 621 dependencies
                    if let Some(deps_arr) = toml_val
                        .get("project")
                        .and_then(|p| p.get("dependencies"))
                        .and_then(|d| d.as_array())
                    {
                        for dep in deps_arr {
                            if let Some(s) = dep.as_str() {
                                let (name, version) = Self::parse_pep508(s);
                                deps.push(DepInfo {
                                    name,
                                    ecosystem: Ecosystem::Pip,
                                    current_version: version,
                                    latest_version: None,
                                    update_available: false,
                                    severity: UpdateSeverity::Info,
                                    checked_at,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Fallback to requirements.txt
        let req_path = path.join("requirements.txt");
        if req_path.exists() && deps.is_empty() {
            if let Ok(content) = std::fs::read_to_string(&req_path) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
                        continue;
                    }
                    let (name, version) = Self::parse_requirement_line(trimmed);
                    if !name.is_empty() {
                        deps.push(DepInfo {
                            name,
                            ecosystem: Ecosystem::Pip,
                            current_version: version,
                            latest_version: None,
                            update_available: false,
                            severity: UpdateSeverity::Info,
                            checked_at,
                        });
                    }
                }
            }
        }

        deps
    }

    fn parse_composer(path: &Path) -> Vec<DepInfo> {
        let manifest_path = path.join("composer.json");
        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read composer.json: {}", e);
                return Vec::new();
            }
        };

        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => {
                debug!("Failed to parse composer.json: {}", e);
                return Vec::new();
            }
        };

        let mut deps = Vec::new();
        let checked_at = Utc::now();

        for section in &["require", "require-dev"] {
            if let Some(obj) = json.get(section).and_then(|v| v.as_object()) {
                for (name, version_val) in obj {
                    if name == "php" || name.starts_with("ext-") {
                        continue; // Skip PHP version and extension requirements
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
                        checked_at,
                    });
                }
            }
        }

        deps
    }

    fn parse_gem(path: &Path) -> Vec<DepInfo> {
        let gemfile_path = path.join("Gemfile");
        let content = match std::fs::read_to_string(&gemfile_path) {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read Gemfile: {}", e);
                return Vec::new();
            }
        };

        let mut deps = Vec::new();
        let checked_at = Utc::now();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || trimmed.starts_with('#')
                || trimmed.starts_with("source")
                || trimmed.starts_with("group")
                || trimmed.starts_with("end")
            {
                continue;
            }
            // Parse: gem "name", "~> 1.0"
            if let Some(rest) = trimmed.strip_prefix("gem ") {
                let parts: Vec<&str> = rest.split(',').collect();
                if let Some(name_part) = parts.first() {
                    let name = name_part
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();
                    let version = parts
                        .get(1)
                        .map(|v| {
                            v.trim()
                                .trim_matches('"')
                                .trim_matches('\'')
                                .trim_start_matches("~> ")
                                .trim_start_matches(">= ")
                                .trim_start_matches("=")
                                .to_string()
                        })
                        .unwrap_or_else(|| "*".to_string());

                    if !name.is_empty() {
                        deps.push(DepInfo {
                            name,
                            ecosystem: Ecosystem::Gem,
                            current_version: version,
                            latest_version: None,
                            update_available: false,
                            severity: UpdateSeverity::Info,
                            checked_at,
                        });
                    }
                }
            }
        }

        deps
    }

    /// Parse PEP 508 dependency specifier: "package>=1.0,<2.0" → ("package", "1.0")
    fn parse_pep508(spec: &str) -> (String, String) {
        let spec = spec.trim();
        // Find first version specifier character
        let split_pos = spec
            .find(|c: char| c == '=' || c == '>' || c == '<' || c == '!' || c == '~')
            .unwrap_or(spec.len());
        let name = spec[..split_pos].trim().to_string();
        let version_part = spec[split_pos..]
            .trim()
            .trim_start_matches(">=")
            .trim_start_matches("~=")
            .trim_start_matches("==")
            .trim_start_matches('>')
            .trim_start_matches('=')
            .split(',')
            .next()
            .unwrap_or("*")
            .trim()
            .to_string();
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

use crate::core::{
    AppError, DepInfo, Ecosystem, RegistryUrls, Result, UpdateSeverity, bounded_concurrency,
};
use crate::deps::ManifestError;
use chrono::Utc;
use futures_util::future::join_all;
use reqwest::Client;
use semver::Version;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct CheckIssue {
    pub package: String,
    pub ecosystem: Ecosystem,
    pub dev_dependency: bool,
    pub manifest_name: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct BatchCheck {
    pub dependencies: Vec<DepInfo>,
    pub errors: Vec<CheckIssue>,
    pub manifest_errors: Vec<ManifestError>,
    pub non_public_skipped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VulnerabilitySeverity {
    Unknown,
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for VulnerabilitySeverity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Unknown => "Unknown",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Critical => "Critical",
        };
        formatter.write_str(label)
    }
}

#[derive(Debug, Clone)]
pub struct Vulnerability {
    pub id: String,
    pub aliases: Vec<String>,
    pub severity: VulnerabilitySeverity,
    pub summary: String,
}

#[derive(Debug, serde::Deserialize)]
struct OsvResponse {
    #[serde(default)]
    vulns: Vec<OsvVulnerability>,
}

#[derive(Debug, serde::Deserialize)]
struct OsvVulnerability {
    id: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    database_specific: Option<OsvDatabaseSpecific>,
    #[serde(default)]
    summary: String,
}

#[derive(Debug, serde::Deserialize)]
struct OsvDatabaseSpecific {
    severity: Option<String>,
}

pub fn highest_vulnerability_severity(vulnerabilities: &[Vulnerability]) -> VulnerabilitySeverity {
    vulnerabilities
        .iter()
        .map(|vulnerability| vulnerability.severity)
        .max()
        .unwrap_or(VulnerabilitySeverity::Unknown)
}

/// Hybrid registry client: checks local cache first, then polls registries event-driven.
pub struct RegistryClient {
    client: Client,
    urls: RegistryUrls,
    air_gap_mode: bool,
}

impl RegistryClient {
    pub fn air_gap_mode(&self) -> bool {
        self.air_gap_mode
    }

    pub fn new(urls: RegistryUrls, air_gap_mode: bool) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .user_agent(concat!("appchecker/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            urls,
            air_gap_mode,
        }
    }

    pub async fn check_update(&self, dep: &DepInfo) -> Result<DepInfo> {
        let (updated, error) = self.check_update_detailed(dep).await;
        if let Some(error) = error {
            warn!(
                "Registry check failed for {} ({}): {}",
                error.package, error.ecosystem, error.message
            );
        }
        Ok(updated)
    }

    pub async fn check_update_detailed(&self, dep: &DepInfo) -> (DepInfo, Option<CheckIssue>) {
        if self.air_gap_mode {
            debug!("Air-gap mode: skipping registry check for {}", dep.name);
            return (dep.clone(), None);
        }

        let mut updated = dep.clone();
        updated.checked_at = Utc::now();
        if !is_public_dependency(dep) {
            return (
                updated,
                Some(CheckIssue {
                    package: dep.name.clone(),
                    ecosystem: dep.ecosystem,
                    dev_dependency: dep.dev_dependency,
                    manifest_name: dep.manifest_name.clone(),
                    message: "not a public registry dependency".into(),
                }),
            );
        }

        let latest = match dep.ecosystem {
            Ecosystem::Npm => self.check_npm(&dep.name).await,
            Ecosystem::Cargo => self.check_crates_io(&dep.name).await,
            Ecosystem::Pip => self.check_pypi(&dep.name).await,
            Ecosystem::Go => self.check_go_proxy(&dep.name).await,
            Ecosystem::Composer => self.check_packagist(&dep.name).await,
            Ecosystem::Gem => self.check_rubygems(&dep.name).await,
        };

        match latest {
            Ok(latest_ver) => {
                updated.latest_version = Some(latest_ver.clone());
                updated.update_available = Self::is_newer(&dep.current_version, &latest_ver);
                updated.severity = if updated.update_available {
                    Self::classify_severity(&dep.current_version, &latest_ver)
                } else {
                    UpdateSeverity::Info
                };
                (updated, None)
            }
            Err(error) => (
                updated,
                Some(CheckIssue {
                    package: dep.name.clone(),
                    ecosystem: dep.ecosystem,
                    dev_dependency: dep.dev_dependency,
                    manifest_name: dep.manifest_name.clone(),
                    message: error.to_string(),
                }),
            ),
        }
    }

    pub async fn check_batch(&self, deps: &[DepInfo], max_concurrent: usize) -> Vec<DepInfo> {
        let result = self.check_batch_detailed(deps, max_concurrent).await;
        for issue in &result.errors {
            warn!(
                "Registry {} ({}): {}",
                issue.package, issue.ecosystem, issue.message
            );
        }
        result.dependencies
    }

    pub async fn check_batch_detailed(
        &self,
        deps: &[DepInfo],
        max_concurrent: usize,
    ) -> BatchCheck {
        self.check_batch_detailed_with_limit(deps, max_concurrent, None)
            .await
    }

    pub async fn check_batch_detailed_with_limit(
        &self,
        deps: &[DepInfo],
        max_concurrent: usize,
        shared_semaphore: Option<Arc<Semaphore>>,
    ) -> BatchCheck {
        let semaphore = shared_semaphore
            .unwrap_or_else(|| Arc::new(Semaphore::new(bounded_concurrency(max_concurrent))));
        let futures: Vec<_> = deps
            .iter()
            .map(|dep| {
                let sem = semaphore.clone();
                let dep = dep.clone();
                async move {
                    let _permit = sem.acquire().await.ok();
                    self.check_update_detailed(&dep).await
                }
            })
            .collect();

        let (dependencies, errors): (Vec<_>, Vec<_>) = join_all(futures).await.into_iter().unzip();
        BatchCheck {
            dependencies,
            errors: errors.into_iter().flatten().collect(),
            manifest_errors: Vec::new(),
            non_public_skipped: false,
        }
    }

    async fn check_npm(&self, name: &str) -> Result<String> {
        let url = format!("{}/{}", self.urls.npm, name);
        let resp: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let latest = resp["dist-tags"]["latest"]
            .as_str()
            .ok_or_else(|| AppError::Registry(format!("No latest tag for npm package {}", name)))?;
        Ok(latest.to_string())
    }

    async fn check_crates_io(&self, name: &str) -> Result<String> {
        let url = format!("{}/crates/{}", self.urls.crates_io, name);
        let resp: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let latest = resp["crate"]["max_stable_version"]
            .as_str()
            .or_else(|| resp["crate"]["max_version"].as_str())
            .ok_or_else(|| AppError::Registry(format!("No version for crate {}", name)))?;
        Ok(latest.to_string())
    }

    async fn check_pypi(&self, name: &str) -> Result<String> {
        let url = format!("{}/{}/json", self.urls.pypi, name);
        let resp: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let latest = resp["info"]["version"]
            .as_str()
            .ok_or_else(|| AppError::Registry(format!("No version for pypi package {}", name)))?;
        Ok(latest.to_string())
    }

    async fn check_go_proxy(&self, name: &str) -> Result<String> {
        let url = format!(
            "{}/{}/@latest",
            self.urls.go_proxy,
            escape_go_module_path(name)
        );
        let resp: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let version = resp["Version"]
            .as_str()
            .ok_or_else(|| AppError::Registry(format!("No version for go module {}", name)))?
            .trim_start_matches('v');
        Ok(version.to_string())
    }

    async fn check_packagist(&self, name: &str) -> Result<String> {
        let url = format!("{}/p2/{}.json", self.urls.packagist, name);
        let resp: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let packages = resp["packages"][name]
            .as_array()
            .ok_or_else(|| AppError::Registry(format!("No packages for composer {}", name)))?;
        let latest = packages
            .first()
            .and_then(|p| p["version"].as_str())
            .ok_or_else(|| AppError::Registry(format!("No version for composer {}", name)))?
            .trim_start_matches('v');
        Ok(latest.to_string())
    }

    async fn check_rubygems(&self, name: &str) -> Result<String> {
        let url = format!("{}/gems/{}.json", self.urls.rubygems, name);
        let resp: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let latest = resp["version"]
            .as_str()
            .ok_or_else(|| AppError::Registry(format!("No version for gem {}", name)))?;
        Ok(latest.to_string())
    }

    pub fn is_comparable(version: &str) -> bool {
        parse_loose_version(version).is_some()
    }

    pub async fn query_vulnerabilities(&self, dependency: &DepInfo) -> Result<Vec<Vulnerability>> {
        if self.air_gap_mode {
            return Err(AppError::Other(
                "OSV vulnerability checks are disabled in air-gap mode".into(),
            ));
        }
        if !is_public_dependency(dependency) {
            return Err(AppError::Other(
                "OSV audit is available only for public registry dependencies".into(),
            ));
        }
        let ecosystem = match dependency.ecosystem {
            Ecosystem::Npm => "npm",
            Ecosystem::Cargo => "crates.io",
            Ecosystem::Pip => "PyPI",
            Ecosystem::Go => "Go",
            Ecosystem::Composer => "Packagist",
            Ecosystem::Gem => "RubyGems",
        };
        let response = self
            .client
            .post(format!("{}/v1/query", self.urls.osv.trim_end_matches('/')))
            .json(&serde_json::json!({
                "package": {
                    "name": &dependency.name,
                    "ecosystem": ecosystem,
                },
                "version": &dependency.current_version,
            }))
            .send()
            .await?
            .error_for_status()?
            .json::<OsvResponse>()
            .await?;
        Ok(response
            .vulns
            .into_iter()
            .map(|vulnerability| {
                let severity = vulnerability
                    .database_specific
                    .and_then(|specific| specific.severity)
                    .map(|severity| match severity.to_ascii_uppercase().as_str() {
                        "CRITICAL" => VulnerabilitySeverity::Critical,
                        "HIGH" => VulnerabilitySeverity::High,
                        "MEDIUM" | "MODERATE" => VulnerabilitySeverity::Medium,
                        "LOW" => VulnerabilitySeverity::Low,
                        _ => VulnerabilitySeverity::Unknown,
                    })
                    .unwrap_or(VulnerabilitySeverity::Unknown);
                Vulnerability {
                    id: vulnerability.id,
                    aliases: vulnerability.aliases,
                    severity,
                    summary: vulnerability.summary,
                }
            })
            .collect())
    }

    pub async fn query_vulnerability_batch(
        &self,
        dependencies: &[DepInfo],
        max_concurrent: usize,
    ) -> Vec<(String, Result<Vec<Vulnerability>>)> {
        let semaphore = Arc::new(Semaphore::new(bounded_concurrency(max_concurrent)));
        let checks = dependencies.iter().map(|dependency| {
            let semaphore = semaphore.clone();
            async move {
                let _permit = semaphore.acquire().await.expect("OSV semaphore closed");
                (
                    dependency.name.clone(),
                    self.query_vulnerabilities(dependency).await,
                )
            }
        });
        join_all(checks).await
    }

    pub fn is_newer(current: &str, latest: &str) -> bool {
        match (parse_loose_version(current), parse_loose_version(latest)) {
            (Some(current), Some(latest)) => latest > current,
            _ => false,
        }
    }

    fn classify_severity(current: &str, latest: &str) -> UpdateSeverity {
        let current_v = parse_loose_version(current);
        let latest_v = parse_loose_version(latest);
        match (current_v, latest_v) {
            (Some(c), Some(l)) => {
                if l.major > c.major {
                    UpdateSeverity::Major
                } else if l.minor > c.minor {
                    UpdateSeverity::Minor
                } else {
                    UpdateSeverity::Info
                }
            }
            _ => UpdateSeverity::Minor, // Default to minor when we can't parse
        }
    }
}

fn escape_go_module_path(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_uppercase() {
                format!("!{}", character.to_ascii_lowercase())
            } else {
                character.to_string()
            }
        })
        .collect()
}

fn is_public_dependency(dependency: &DepInfo) -> bool {
    let name = dependency.name.trim();
    if name.is_empty() || name.contains("://") || name.contains("..") {
        return false;
    }
    let segment = |value: &str| {
        !value.is_empty()
            && value
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    };
    match dependency.ecosystem {
        Ecosystem::Npm => {
            let scoped = name.starts_with('@');
            let path = name.strip_prefix('@').unwrap_or(name);
            let parts: Vec<&str> = path.split('/').collect();
            if scoped {
                parts.len() == 2 && parts.iter().all(|part| segment(part))
            } else {
                parts.len() == 1 && segment(parts[0])
            }
        }
        Ecosystem::Cargo | Ecosystem::Pip | Ecosystem::Gem => segment(name),
        Ecosystem::Composer => {
            let mut parts = name.split('/');
            matches!((parts.next(), parts.next(), parts.next()), (Some(vendor), Some(package), None) if segment(vendor) && segment(package))
        }
        Ecosystem::Go => {
            name.split('/')
                .next()
                .map(|domain| domain.contains('.') && domain != ".")
                .unwrap_or(false)
                && !name.split('/').any(|part| part == ".")
                && !name.starts_with('/')
                && !name.ends_with('/')
                && !name.contains("//")
                && name.split('/').all(segment)
        }
    }
}

fn parse_loose_version(version: &str) -> Option<Version> {
    let mut value = version.trim();
    if value.starts_with('<') {
        return None;
    }
    for prefix in [">=", "<=", "^", "~", ">", "<", "=", "v"] {
        if let Some(stripped) = value.strip_prefix(prefix) {
            value = stripped.trim();
            break;
        }
    }
    if value.is_empty() || value == "*" {
        return None;
    }
    let (suffix, core) = value
        .find(['-', '+'])
        .map(|index| (&value[index..], &value[..index]))
        .unwrap_or(("", value));
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() > 3 || parts.iter().any(|part| part.parse::<u64>().is_err()) {
        return None;
    }
    let mut normalized = parts.join(".");
    for _ in parts.len()..3 {
        normalized.push_str(".0");
    }
    normalized.push_str(suffix);
    Version::parse(&normalized).ok()
}

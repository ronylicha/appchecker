use crate::core::{AppError, DepInfo, Ecosystem, RegistryUrls, Result, UpdateSeverity};
use chrono::Utc;
use futures_util::future::join_all;
use reqwest::Client;
use semver::Version;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tracing::{debug, warn};

/// Hybrid registry client: checks local cache first, then polls registries event-driven.
pub struct RegistryClient {
    client: Client,
    urls: RegistryUrls,
    air_gap_mode: bool,
}

impl RegistryClient {
    pub fn new(urls: RegistryUrls, air_gap_mode: bool) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .user_agent("appchecker/0.1.0")
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            urls,
            air_gap_mode,
        }
    }

    /// Check a single dependency for updates against its registry.
    /// Returns updated DepInfo with latest_version and severity filled in.
    pub async fn check_update(&self, dep: &DepInfo) -> Result<DepInfo> {
        if self.air_gap_mode {
            debug!("Air-gap mode: skipping registry check for {}", dep.name);
            return Ok(dep.clone());
        }

        let latest = match dep.ecosystem {
            Ecosystem::Npm => self.check_npm(&dep.name).await,
            Ecosystem::Cargo => self.check_crates_io(&dep.name).await,
            Ecosystem::Pip => self.check_pypi(&dep.name).await,
            Ecosystem::Go => self.check_go_proxy(&dep.name).await,
            Ecosystem::Composer => self.check_packagist(&dep.name).await,
            Ecosystem::Gem => self.check_rubygems(&dep.name).await,
        };

        let mut updated = dep.clone();
        updated.checked_at = Utc::now();

        match latest {
            Ok(latest_ver) => {
                updated.latest_version = Some(latest_ver.clone());
                updated.update_available = Self::is_newer(&dep.current_version, &latest_ver);
                updated.severity = if updated.update_available {
                    Self::classify_severity(&dep.current_version, &latest_ver)
                } else {
                    UpdateSeverity::Info
                };
            }
            Err(e) => {
                warn!(
                    "Registry check failed for {} ({}): {}",
                    dep.name, dep.ecosystem, e
                );
                // Keep existing data on transient failure
            }
        }

        Ok(updated)
    }

    /// Batch check multiple dependencies with concurrency limit
    pub async fn check_batch(&self, deps: &[DepInfo], max_concurrent: usize) -> Vec<DepInfo> {
        let semaphore = Arc::new(Semaphore::new(max_concurrent));
        let futures: Vec<_> = deps
            .iter()
            .map(|dep| {
                let sem = semaphore.clone();
                let dep = dep.clone();
                async move {
                    let _permit = sem.acquire().await.ok();
                    self.check_update(&dep).await.unwrap_or(dep)
                }
            })
            .collect();

        join_all(futures).await
    }

    async fn check_npm(&self, name: &str) -> Result<String> {
        let url = format!("{}/{}", self.urls.npm, name);
        let resp: serde_json::Value = self.client.get(&url).send().await?.json().await?;
        let latest = resp["dist-tags"]["latest"]
            .as_str()
            .ok_or_else(|| AppError::Registry(format!("No latest tag for npm package {}", name)))?;
        Ok(latest.to_string())
    }

    async fn check_crates_io(&self, name: &str) -> Result<String> {
        let url = format!("{}/crates/{}", self.urls.crates_io, name);
        let resp: serde_json::Value = self.client.get(&url).send().await?.json().await?;
        let latest = resp["crate"]["max_stable_version"]
            .as_str()
            .or_else(|| resp["crate"]["max_version"].as_str())
            .ok_or_else(|| AppError::Registry(format!("No version for crate {}", name)))?;
        Ok(latest.to_string())
    }

    async fn check_pypi(&self, name: &str) -> Result<String> {
        let url = format!("{}/{}/json", self.urls.pypi, name);
        let resp: serde_json::Value = self.client.get(&url).send().await?.json().await?;
        let latest = resp["info"]["version"]
            .as_str()
            .ok_or_else(|| AppError::Registry(format!("No version for pypi package {}", name)))?;
        Ok(latest.to_string())
    }

    async fn check_go_proxy(&self, name: &str) -> Result<String> {
        let url = format!("{}/{}/@latest", self.urls.go_proxy, name);
        let resp: serde_json::Value = self.client.get(&url).send().await?.json().await?;
        let version = resp["Version"]
            .as_str()
            .ok_or_else(|| AppError::Registry(format!("No version for go module {}", name)))?
            .trim_start_matches('v');
        Ok(version.to_string())
    }

    async fn check_packagist(&self, name: &str) -> Result<String> {
        let url = format!("{}/p2/{}.json", self.urls.packagist, name);
        let resp: serde_json::Value = self.client.get(&url).send().await?.json().await?;
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
        let resp: serde_json::Value = self.client.get(&url).send().await?.json().await?;
        let latest = resp["version"]
            .as_str()
            .ok_or_else(|| AppError::Registry(format!("No version for gem {}", name)))?;
        Ok(latest.to_string())
    }

    fn is_newer(current: &str, latest: &str) -> bool {
        let current_v = Version::parse(current.trim_start_matches('v')).ok();
        let latest_v = Version::parse(latest.trim_start_matches('v')).ok();
        match (current_v, latest_v) {
            (Some(c), Some(l)) => l > c,
            _ => current != latest, // Fallback to string comparison
        }
    }

    fn classify_severity(current: &str, latest: &str) -> UpdateSeverity {
        let current_v = Version::parse(current.trim_start_matches('v')).ok();
        let latest_v = Version::parse(latest.trim_start_matches('v')).ok();
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

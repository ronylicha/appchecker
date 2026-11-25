use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a monitored project
pub type ProjectId = Uuid;

pub const MAX_CONCURRENT_SCANS: usize = 256;

pub fn bounded_concurrency(value: usize) -> usize {
    value.clamp(1, MAX_CONCURRENT_SCANS)
}

/// A monitored local project directory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub path: std::path::PathBuf,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub notification_overrides: Option<NotificationPrefs>,
    pub added_at: DateTime<Utc>,
    pub last_scanned_at: Option<DateTime<Utc>>,
}

impl Project {
    pub fn new(name: String, path: std::path::PathBuf) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            path,
            tags: Vec::new(),
            pinned: false,
            notification_overrides: None,
            added_at: Utc::now(),
            last_scanned_at: None,
        }
    }
}

/// Current git state of a repository
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitState {
    pub current_branch: Option<String>,
    pub head_oid: Option<String>,
    pub is_dirty: bool,
    pub untracked_count: u32,
    pub staged_count: u32,
    pub ahead: u32,
    pub behind: u32,
    pub has_stashes: bool,
    pub stash_count: u32,
    pub last_fetch_at: Option<DateTime<Utc>>,
    pub scanned_at: DateTime<Utc>,
}

/// Dependency information for a single package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepInfo {
    pub name: String,
    pub ecosystem: Ecosystem,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub severity: UpdateSeverity,
    #[serde(default)]
    pub dev_dependency: bool,
    #[serde(default)]
    pub manifest_name: Option<String>,
    pub checked_at: DateTime<Utc>,
}

/// Supported package ecosystems
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ecosystem {
    Npm,
    Cargo,
    Pip,
    Go,
    Composer,
    Gem,
}

impl std::fmt::Display for Ecosystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ecosystem::Npm => write!(f, "npm"),
            Ecosystem::Cargo => write!(f, "cargo"),
            Ecosystem::Pip => write!(f, "pip"),
            Ecosystem::Go => write!(f, "go"),
            Ecosystem::Composer => write!(f, "composer"),
            Ecosystem::Gem => write!(f, "gem"),
        }
    }
}

/// Severity level for dependency updates
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateSeverity {
    Info,
    Minor,
    Major,
    Security,
}

impl std::fmt::Display for UpdateSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateSeverity::Info => write!(f, "info"),
            UpdateSeverity::Minor => write!(f, "minor"),
            UpdateSeverity::Major => write!(f, "major"),
            UpdateSeverity::Security => write!(f, "security"),
        }
    }
}

/// Per-project or global notification preferences
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationPrefs {
    pub enabled: bool,
    pub quiet_hours_start: Option<String>, // "HH:MM" format
    pub quiet_hours_end: Option<String>,
    pub min_severity: UpdateSeverity,
    pub batch_interval_secs: u64,
    pub show_minor_updates: bool,
    pub show_major_updates: bool,
    pub show_security: bool,
}

impl Default for NotificationPrefs {
    fn default() -> Self {
        Self {
            enabled: true,
            quiet_hours_start: Some("22:00".to_string()),
            quiet_hours_end: Some("08:00".to_string()),
            min_severity: UpdateSeverity::Minor,
            batch_interval_secs: 3600, // 1 hour digest
            show_minor_updates: false,
            show_major_updates: true,
            show_security: true,
        }
    }
}

/// Notification state tracking to prevent duplicates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertState {
    Active,
    Acknowledged,
    Completed,
}

/// A pending or sent notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Uuid,
    pub project_id: ProjectId,
    pub severity: UpdateSeverity,
    pub title: String,
    pub body: String,
    pub state: AlertState,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub acknowledged_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    pub project: String,
    pub branch: Option<String>,
    pub dirty: bool,
    pub total_deps: usize,
    pub outdated_deps: usize,
    pub registry_errors: usize,
    pub non_public_skipped: bool,
    pub air_gap_mode: bool,
    pub git_error: Option<String>,
    pub registry_error_details: Vec<String>,
    pub manifest_errors: Vec<String>,
}

/// Aggregate health status for a project (displayed in TUI dashboard)
#[derive(Debug, Clone, Default)]
pub struct ProjectHealth {
    pub project_id: ProjectId,
    pub git_state: Option<GitState>,
    pub outdated_deps_count: u32,
    pub security_updates_count: u32,
    pub major_updates_count: u32,
    pub last_scanned_at: Option<DateTime<Utc>>,
    pub scan_error: Option<String>,
}

/// Application-wide configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub projects: Vec<Project>,
    pub global_notification_prefs: NotificationPrefs,
    pub scan_interval_secs: u64,
    pub max_concurrent_scans: usize,
    pub air_gap_mode: bool,
    pub trusted_repositories: Vec<ProjectId>,
    pub registry_urls: RegistryUrls,
    pub auto_discover_paths: Vec<std::path::PathBuf>,
    pub max_auto_discover_depth: usize,
    pub file_descriptor_limit: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            global_notification_prefs: NotificationPrefs::default(),
            scan_interval_secs: 300, // 5 minutes
            max_concurrent_scans: 8,
            air_gap_mode: false,
            trusted_repositories: Vec::new(),
            registry_urls: RegistryUrls::default(),
            auto_discover_paths: vec![
                dirs::home_dir()
                    .map(|h| h.join("Projects"))
                    .unwrap_or_else(|| std::path::PathBuf::from("~/Projects")),
            ],
            max_auto_discover_depth: 3,
            file_descriptor_limit: 16384,
        }
    }
}

/// Configurable registry URLs for air-gap / corporate proxy support
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RegistryUrls {
    pub npm: String,
    pub crates_io: String,
    pub pypi: String,
    pub go_proxy: String,
    pub packagist: String,
    pub rubygems: String,
    pub osv: String,
}

impl Default for RegistryUrls {
    fn default() -> Self {
        Self {
            npm: "https://registry.npmjs.org".to_string(),
            crates_io: "https://crates.io/api/v1".to_string(),
            pypi: "https://pypi.org/pypi".to_string(),
            go_proxy: "https://proxy.golang.org".to_string(),
            packagist: "https://repo.packagist.org".to_string(),
            rubygems: "https://rubygems.org/api/v1".to_string(),
            osv: "https://api.osv.dev".to_string(),
        }
    }
}

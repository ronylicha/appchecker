use crate::core::{AppError, GitState, Result};
use chrono::Utc;
use std::path::Path;
use tracing::{debug, warn};

/// Scans a git repository and returns its current state.
/// Designed to be called via `tokio::task::spawn_blocking` to avoid blocking the async runtime.
pub struct GitScanner;

impl GitScanner {
    /// Scan a single repository at the given path.
    pub fn scan_repo(path: &Path) -> Result<GitState> {
        let scanned_at = Utc::now();

        let repo = gix::open(path)
            .map_err(|e| AppError::Git(format!("Failed to open repo at {:?}: {}", path, e)))?;

        let head_oid = repo
            .head()
            .ok()
            .and_then(|h| h.id())
            .map(|id| id.to_string());

        let current_branch = repo
            .head()
            .ok()
            .and_then(|h| h.referent_name().map(|n| n.shorten().to_string()));

        // Full dirty check using index vs workdir comparison
        let (is_dirty, staged_count, untracked_count) = Self::check_working_tree(&repo);

        // Check ahead/behind against upstream tracking branch via git CLI
        let (ahead, behind) = Self::check_ahead_behind(path);

        // Check stashes
        let stash_count = Self::count_stashes(&repo);

        Ok(GitState {
            current_branch,
            head_oid,
            is_dirty,
            untracked_count,
            staged_count,
            ahead,
            behind,
            has_stashes: stash_count > 0,
            stash_count,
            last_fetch_at: None,
            scanned_at,
        })
    }

    /// Full working tree status check using gix index iteration.
    /// Returns (is_dirty, staged_count, untracked_count).
    fn check_working_tree(repo: &gix::Repository) -> (bool, u32, u32) {
        let mut is_dirty = false;
        let mut staged_count: u32 = 0;
        let mut untracked_count: u32 = 0;

        let index = match repo.index_or_empty() {
            Ok(idx) => idx,
            Err(e) => {
                warn!("Failed to read index: {}", e);
                return (false, 0, 0);
            }
        };

        // Build set of indexed paths for untracked detection
        let indexed_paths: std::collections::HashSet<String> = index
            .entries()
            .iter()
            .filter_map(|e| {
                std::str::from_utf8(&e.path(&index))
                    .ok()
                    .map(|s| s.to_string())
            })
            .collect();

        // Iterate through index entries to detect staged changes
        for entry in index.entries() {
            let entry_path = match std::str::from_utf8(&entry.path(&index)) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let full_path = match repo.work_dir() {
                Some(wd) => wd.join(entry_path),
                None => continue,
            };

            if !full_path.exists() {
                staged_count += 1;
                is_dirty = true;
            } else if let Ok(metadata) = full_path.metadata() {
                let index_size = entry.stat.size;
                if metadata.len() != index_size as u64 {
                    staged_count += 1;
                    is_dirty = true;
                }
            }
        }

        // Check for untracked files at top level
        if let Some(work_dir) = repo.work_dir() {
            if let Ok(entries) = std::fs::read_dir(work_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();

                    if name_str.starts_with('.')
                        || name_str == "node_modules"
                        || name_str == "target"
                    {
                        continue;
                    }

                    if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                        if !indexed_paths.contains(name_str.as_ref()) {
                            untracked_count += 1;
                            is_dirty = true;
                        }
                    }
                }
            }
        }

        (is_dirty, staged_count, untracked_count)
    }

    /// Calculate ahead/behind counts using git CLI (stable across all git versions).
    fn check_ahead_behind(repo_path: &Path) -> (u32, u32) {
        let output = std::process::Command::new("git")
            .args(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
            .current_dir(repo_path)
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let parts: Vec<&str> = stdout.trim().split('\t').collect();
                if parts.len() == 2 {
                    let ahead = parts[0].parse::<u32>().unwrap_or(0);
                    let behind = parts[1].parse::<u32>().unwrap_or(0);
                    return (ahead, behind);
                }
                (0, 0)
            }
            _ => (0, 0),
        }
    }

    fn count_stashes(repo: &gix::Repository) -> u32 {
        match repo.find_reference("refs/stash") {
            Ok(_) => 1,
            Err(_) => 0,
        }
    }

    /// Async wrapper that spawns the blocking scan on a dedicated thread pool
    pub async fn scan_repo_async(path: std::path::PathBuf) -> Result<GitState> {
        tokio::task::spawn_blocking(move || Self::scan_repo(&path))
            .await
            .map_err(|e| AppError::Other(format!("Task join error: {}", e)))?
    }
}

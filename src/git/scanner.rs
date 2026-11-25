use crate::core::{AppError, GitState, Result};
use chrono::Utc;
use std::path::Path;

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
        let (is_dirty, staged_count, untracked_count) = Self::check_working_tree(&repo)?;

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

    /// Full working tree status using Git's staged and untracked file reporting.
    /// Returns (is_dirty, staged_count, untracked_count).
    fn check_working_tree(repo: &gix::Repository) -> Result<(bool, u32, u32)> {
        let Some(work_dir) = repo.work_dir() else {
            return Err(AppError::Git("Cannot inspect working tree".into()));
        };
        let output = std::process::Command::new("git")
            .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
            .current_dir(work_dir)
            .output()
            .map_err(|error| AppError::Git(format!("Cannot run git status: {error}")))?;
        if !output.status.success() {
            return Err(AppError::Git(format!(
                "Git status failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let mut is_dirty = false;
        let mut staged_count = 0;
        let mut untracked_count = 0;
        let mut entries = output.stdout.split(|byte| *byte == 0);
        while let Some(entry) = entries.next() {
            if entry.len() < 3 || entry[2] != b' ' {
                continue;
            }
            let index_status = entry[0];
            let worktree_status = entry[1];
            if matches!(index_status, b'R' | b'C') || matches!(worktree_status, b'R' | b'C') {
                let _ = entries.next();
            }
            if index_status == b'?' && worktree_status == b'?' {
                untracked_count += 1;
                is_dirty = true;
                continue;
            }
            if index_status == b'!' && worktree_status == b'!' {
                continue;
            }
            if index_status != b' ' || worktree_status != b' ' {
                is_dirty = true;
            }
            if index_status != b' ' {
                staged_count += 1;
            }
        }
        Ok((is_dirty, staged_count, untracked_count))
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

use crate::core::{ProjectId, Result};
use notify_debouncer_mini::{DebouncedEventKind, new_debouncer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

/// Events emitted by the filesystem watcher when monitored paths change.
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A git-related file changed (HEAD, refs, index, etc.)
    GitChanged {
        project_id: ProjectId,
        path: PathBuf,
    },
    /// A dependency manifest changed (package.json, Cargo.toml, etc.)
    ManifestChanged {
        project_id: ProjectId,
        path: PathBuf,
    },
    /// A project directory was removed or became inaccessible
    ProjectLost { project_id: ProjectId },
}

/// Filesystem watcher with debounce, backpressure, and per-project path mapping.
pub struct FsWatcher {
    event_tx: broadcast::Sender<WatchEvent>,
    _keepalive: mpsc::Sender<()>,
}

impl FsWatcher {
    /// Create a new watcher. Returns the watcher and a receiver for events.
    pub fn new(debounce_ms: u64) -> Result<(Self, broadcast::Receiver<WatchEvent>)> {
        let (event_tx, event_rx) = broadcast::channel(256);
        let (_keepalive, _ka_rx) = mpsc::channel::<()>();

        Ok((
            Self {
                event_tx,
                _keepalive,
            },
            event_rx,
        ))
    }

    /// Start watching a set of project directories.
    /// This spawns a background thread that runs the notify debouncer.
    pub fn start_watching(
        &self,
        projects: &[(ProjectId, PathBuf)],
        debounce_ms: u64,
    ) -> Result<()> {
        let event_tx = self.event_tx.clone();

        // Build reverse lookup: path prefix → project_id
        let path_map: HashMap<PathBuf, ProjectId> = projects
            .iter()
            .map(|(id, path)| (path.clone(), *id))
            .collect();

        // Collect all paths to watch
        let watch_paths: Vec<PathBuf> = projects.iter().map(|(_, p)| p.clone()).collect();

        std::thread::spawn(move || {
            let (tx, rx) = mpsc::channel();

            let mut debouncer = match new_debouncer(Duration::from_millis(debounce_ms), tx) {
                Ok(d) => d,
                Err(e) => {
                    warn!("Failed to create debouncer: {}", e);
                    return;
                }
            };

            // Add each project path to the watcher
            for path in &watch_paths {
                if path.exists() {
                    if let Err(e) = debouncer
                        .watcher()
                        .watch(path, notify::RecursiveMode::Recursive)
                    {
                        warn!("Failed to watch {:?}: {}", path, e);
                    } else {
                        debug!("Watching {:?}", path);
                    }
                } else {
                    warn!("Path does not exist, skipping: {:?}", path);
                }
            }

            info!(
                "Filesystem watcher started for {} projects",
                watch_paths.len()
            );

            // Process debounced events
            for result in rx {
                match result {
                    Ok(events) => {
                        for event in events {
                            if event.kind != DebouncedEventKind::Any {
                                continue;
                            }
                            let changed_path = &event.path;

                            // Find which project this path belongs to
                            let project_id = path_map
                                .iter()
                                .find(|(prefix, _)| changed_path.starts_with(prefix))
                                .map(|(_, id)| *id);

                            if let Some(pid) = project_id {
                                let watch_event = if Self::is_git_path(changed_path) {
                                    WatchEvent::GitChanged {
                                        project_id: pid,
                                        path: changed_path.clone(),
                                    }
                                } else if Self::is_manifest_path(changed_path) {
                                    WatchEvent::ManifestChanged {
                                        project_id: pid,
                                        path: changed_path.clone(),
                                    }
                                } else {
                                    continue; // Not a path we care about
                                };

                                // Non-blocking send; drop if no receivers
                                let _ = event_tx.send(watch_event);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Watcher error: {:?}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Check if a changed path is git-related
    fn is_git_path(path: &Path) -> bool {
        let components: Vec<&str> = path
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();

        // Look for .git directory components
        for (i, comp) in components.iter().enumerate() {
            if *comp == ".git" {
                // Check what's inside .git
                if let Some(sub) = components.get(i + 1) {
                    return matches!(
                        *sub,
                        "HEAD" | "refs" | "index" | "logs" | "FETCH_HEAD" | "ORIG_HEAD"
                    );
                }
                return true; // .git itself changed
            }
        }
        false
    }

    /// Check if a changed path is a dependency manifest
    fn is_manifest_path(path: &Path) -> bool {
        path.file_name()
            .and_then(|n| n.to_str())
            .map(|name| {
                matches!(
                    name,
                    "package.json"
                        | "Cargo.toml"
                        | "go.mod"
                        | "pyproject.toml"
                        | "requirements.txt"
                        | "composer.json"
                        | "Gemfile"
                        | "Pipfile"
                        | "poetry.lock"
                        | "Cargo.lock"
                        | "package-lock.json"
                        | "yarn.lock"
                        | "pnpm-lock.yaml"
                )
            })
            .unwrap_or(false)
    }
}

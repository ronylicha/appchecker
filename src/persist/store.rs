use crate::core::{AppError, DepInfo, GitState, Project, ProjectId, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use tracing::{debug, info, warn};

/// Messages envoyés au thread d'écriture DB dédié.
#[derive(Debug)]
pub enum DbCommand {
    SaveGitState {
        project_id: ProjectId,
        state: GitState,
    },
    SaveDeps {
        project_id: ProjectId,
        deps: Vec<DepInfo>,
    },
    UpsertProject(Project),
    Shutdown,
}

/// Handle thread-safe vers le store SQLite.
/// Toutes les écritures passent par un thread dédié propriétaire de la `Connection`,
/// rendant ce type `Send + Sync` pour utilisation dans `tokio::spawn`.
#[derive(Clone)]
pub struct Store {
    cmd_tx: mpsc::Sender<DbCommand>,
    db_path: PathBuf,
}

impl Store {
    /// Ouvre ou crée la base de données et lance le thread d'écriture.
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Migrations synchrones avant de passer la main au thread writer
        {
            let conn = Connection::open(db_path)?;
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
            Self::run_migrations(&conn)?;
        }

        let db_path_owned = db_path.to_path_buf();
        let (cmd_tx, cmd_rx) = mpsc::channel::<DbCommand>();

        thread::Builder::new()
            .name("appchecker-db".into())
            .spawn(move || {
                let conn = match Connection::open(&db_path_owned) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("DB writer failed to open {:?}: {}", db_path_owned, e);
                        return;
                    }
                };
                let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;");

                for cmd in cmd_rx {
                    match cmd {
                        DbCommand::SaveGitState { project_id, state } => {
                            if let Err(e) = Self::do_save_git_state(&conn, project_id, &state) {
                                warn!("save_git_state failed: {}", e);
                            }
                        }
                        DbCommand::SaveDeps { project_id, deps } => {
                            if let Err(e) = Self::do_save_deps(&conn, project_id, &deps) {
                                warn!("save_deps failed: {}", e);
                            }
                        }
                        DbCommand::UpsertProject(proj) => {
                            if let Err(e) = Self::do_upsert_project(&conn, &proj) {
                                warn!("upsert_project failed: {}", e);
                            }
                        }
                        DbCommand::Shutdown => break,
                    }
                }
            })
            .map_err(|e| AppError::Other(format!("Failed to spawn DB writer: {}", e)))?;

        Ok(Self {
            cmd_tx,
            db_path: db_path.to_path_buf(),
        })
    }

    fn run_migrations(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                tags TEXT NOT NULL DEFAULT '[]',
                pinned INTEGER NOT NULL DEFAULT 0,
                notification_overrides TEXT,
                added_at TEXT NOT NULL,
                last_scanned_at TEXT
            );

            CREATE TABLE IF NOT EXISTS git_state (
                project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
                current_branch TEXT,
                head_oid TEXT,
                is_dirty INTEGER NOT NULL DEFAULT 0,
                untracked_count INTEGER NOT NULL DEFAULT 0,
                staged_count INTEGER NOT NULL DEFAULT 0,
                ahead INTEGER NOT NULL DEFAULT 0,
                behind INTEGER NOT NULL DEFAULT 0,
                has_stashes INTEGER NOT NULL DEFAULT 0,
                stash_count INTEGER NOT NULL DEFAULT 0,
                last_fetch_at TEXT,
                scanned_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS deps (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                ecosystem TEXT NOT NULL,
                current_version TEXT NOT NULL,
                latest_version TEXT,
                update_available INTEGER NOT NULL DEFAULT 0,
                severity TEXT NOT NULL DEFAULT 'info',
                checked_at TEXT NOT NULL,
                UNIQUE(project_id, name, ecosystem)
            );

            CREATE INDEX IF NOT EXISTS idx_deps_project ON deps(project_id);
            CREATE INDEX IF NOT EXISTS idx_deps_severity ON deps(severity);
            ",
        )?;
        info!("Database migrations applied");
        Ok(())
    }

    // ── Public async-safe API (send commands to writer thread) ──────

    pub fn upsert_project(&self, project: &Project) -> Result<()> {
        self.cmd_tx
            .send(DbCommand::UpsertProject(project.clone()))
            .map_err(|e| AppError::Other(format!("DB channel closed: {}", e)))
    }

    pub fn save_git_state(&self, project_id: ProjectId, state: &GitState) -> Result<()> {
        self.cmd_tx
            .send(DbCommand::SaveGitState {
                project_id,
                state: state.clone(),
            })
            .map_err(|e| AppError::Other(format!("DB channel closed: {}", e)))
    }

    pub fn save_deps(&self, project_id: ProjectId, deps: &[DepInfo]) -> Result<()> {
        self.cmd_tx
            .send(DbCommand::SaveDeps {
                project_id,
                deps: deps.to_vec(),
            })
            .map_err(|e| AppError::Other(format!("DB channel closed: {}", e)))
    }

    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(DbCommand::Shutdown);
    }

    // ── Synchronous read API (safe: reads don't mutate Connection) ──
    // These open a separate read-only connection for queries.

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, name, path, tags, pinned, notification_overrides, added_at, last_scanned_at FROM projects ORDER BY pinned DESC, name ASC",
        )?;

        let projects = stmt
            .query_map([], |row| {
                let id_str: String = row.get(0)?;
                let name: String = row.get(1)?;
                let path_str: String = row.get(2)?;
                let tags_json: String = row.get(3)?;
                let pinned: i32 = row.get(4)?;
                let overrides_json: Option<String> = row.get(5)?;
                let added_at_str: String = row.get(6)?;
                let last_scanned_str: Option<String> = row.get(7)?;

                Ok(Project {
                    id: uuid::Uuid::parse_str(&id_str).unwrap_or_default(),
                    name,
                    path: PathBuf::from(path_str),
                    tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                    pinned: pinned != 0,
                    notification_overrides: overrides_json
                        .and_then(|j| serde_json::from_str(&j).ok()),
                    added_at: DateTime::parse_from_rfc3339(&added_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    last_scanned_at: last_scanned_str.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .ok()
                    }),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    pub fn get_git_state(&self, project_id: ProjectId) -> Result<Option<GitState>> {
        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT current_branch, head_oid, is_dirty, untracked_count, staged_count, ahead, behind, has_stashes, stash_count, last_fetch_at, scanned_at FROM git_state WHERE project_id = ?1",
        )?;

        let result = stmt.query_row(params![project_id.to_string()], |row| {
            let scanned_at_str: String = row.get(10)?;
            let last_fetch_str: Option<String> = row.get(9)?;

            Ok(GitState {
                current_branch: row.get(0)?,
                head_oid: row.get(1)?,
                is_dirty: row.get::<_, i32>(2)? != 0,
                untracked_count: row.get(3)?,
                staged_count: row.get(4)?,
                ahead: row.get(5)?,
                behind: row.get(6)?,
                has_stashes: row.get::<_, i32>(7)? != 0,
                stash_count: row.get(8)?,
                last_fetch_at: last_fetch_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
                scanned_at: DateTime::parse_from_rfc3339(&scanned_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        });

        match result {
            Ok(state) => Ok(Some(state)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e)),
        }
    }

    pub fn get_outdated_deps(&self, project_id: ProjectId) -> Result<Vec<DepInfo>> {
        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT name, ecosystem, current_version, latest_version, update_available, severity, checked_at
             FROM deps WHERE project_id = ?1 AND update_available = 1
             ORDER BY CASE severity WHEN 'security' THEN 0 WHEN 'major' THEN 1 WHEN 'minor' THEN 2 ELSE 3 END",
        )?;

        let deps = stmt
            .query_map(params![project_id.to_string()], |row| {
                let checked_at_str: String = row.get(6)?;
                Ok(DepInfo {
                    name: row.get(0)?,
                    ecosystem: row
                        .get::<_, String>(1)?
                        .parse()
                        .unwrap_or(crate::core::Ecosystem::Npm),
                    current_version: row.get(2)?,
                    latest_version: row.get(3)?,
                    update_available: row.get::<_, i32>(4)? != 0,
                    severity: row
                        .get::<_, String>(5)?
                        .parse()
                        .unwrap_or(crate::core::UpdateSeverity::Info),
                    checked_at: DateTime::parse_from_rfc3339(&checked_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(deps)
    }

    pub fn get_project_health_summary(&self, project_id: ProjectId) -> Result<(u32, u32, u32)> {
        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT
                COUNT(*) FILTER (WHERE update_available = 1) as outdated,
                COUNT(*) FILTER (WHERE severity = 'security' AND update_available = 1) as security,
                COUNT(*) FILTER (WHERE severity = 'major' AND update_available = 1) as major
             FROM deps WHERE project_id = ?1",
        )?;

        let (outdated, security, major) = stmt
            .query_row(params![project_id.to_string()], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;

        Ok((outdated, security, major))
    }

    pub fn remove_project(&self, id: ProjectId) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;
        conn.execute(
            "DELETE FROM projects WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    // ── Private sync implementations (called from writer thread) ────

    fn do_upsert_project(conn: &Connection, project: &Project) -> Result<()> {
        let tags_json = serde_json::to_string(&project.tags).unwrap_or_else(|_| "[]".into());
        let overrides_json = project
            .notification_overrides
            .as_ref()
            .and_then(|o| serde_json::to_string(o).ok());

        conn.execute(
            "INSERT INTO projects (id, name, path, tags, pinned, notification_overrides, added_at, last_scanned_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(path) DO UPDATE SET
                name = excluded.name,
                tags = excluded.tags,
                pinned = excluded.pinned,
                notification_overrides = excluded.notification_overrides",
            params![
                project.id.to_string(),
                project.name,
                project.path.to_string_lossy(),
                tags_json,
                project.pinned as i32,
                overrides_json,
                project.added_at.to_rfc3339(),
                project.last_scanned_at.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    fn do_save_git_state(conn: &Connection, project_id: ProjectId, state: &GitState) -> Result<()> {
        conn.execute(
            "INSERT INTO git_state (project_id, current_branch, head_oid, is_dirty, untracked_count, staged_count, ahead, behind, has_stashes, stash_count, last_fetch_at, scanned_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(project_id) DO UPDATE SET
                current_branch = excluded.current_branch,
                head_oid = excluded.head_oid,
                is_dirty = excluded.is_dirty,
                untracked_count = excluded.untracked_count,
                staged_count = excluded.staged_count,
                ahead = excluded.ahead,
                behind = excluded.behind,
                has_stashes = excluded.has_stashes,
                stash_count = excluded.stash_count,
                last_fetch_at = excluded.last_fetch_at,
                scanned_at = excluded.scanned_at",
            params![
                project_id.to_string(),
                state.current_branch,
                state.head_oid,
                state.is_dirty as i32,
                state.untracked_count,
                state.staged_count,
                state.ahead,
                state.behind,
                state.has_stashes as i32,
                state.stash_count,
                state.last_fetch_at.map(|t| t.to_rfc3339()),
                state.scanned_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    fn do_save_deps(conn: &Connection, project_id: ProjectId, deps: &[DepInfo]) -> Result<()> {
        let tx = conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO deps (project_id, name, ecosystem, current_version, latest_version, update_available, severity, checked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(project_id, name, ecosystem) DO UPDATE SET
                    current_version = excluded.current_version,
                    latest_version = excluded.latest_version,
                    update_available = excluded.update_available,
                    severity = excluded.severity,
                    checked_at = excluded.checked_at",
            )?;

            for dep in deps {
                stmt.execute(params![
                    project_id.to_string(),
                    dep.name,
                    dep.ecosystem.to_string(),
                    dep.current_version,
                    dep.latest_version,
                    dep.update_available as i32,
                    dep.severity.to_string(),
                    dep.checked_at.to_rfc3339(),
                ])?;
            }
        }
        tx.commit()?;
        debug!("Saved {} deps for project {}", deps.len(), project_id);
        Ok(())
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(DbCommand::Shutdown);
    }
}

// Parse implementations for string ↔ enum conversion
impl std::str::FromStr for crate::core::Ecosystem {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "npm" => Ok(Self::Npm),
            "cargo" => Ok(Self::Cargo),
            "pip" => Ok(Self::Pip),
            "go" => Ok(Self::Go),
            "composer" => Ok(Self::Composer),
            "gem" => Ok(Self::Gem),
            other => Err(format!("Unknown ecosystem: {}", other)),
        }
    }
}

impl std::str::FromStr for crate::core::UpdateSeverity {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "info" => Ok(Self::Info),
            "minor" => Ok(Self::Minor),
            "major" => Ok(Self::Major),
            "security" => Ok(Self::Security),
            other => Err(format!("Unknown severity: {}", other)),
        }
    }
}

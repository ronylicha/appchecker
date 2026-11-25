use appchecker::core::{DepInfo, Ecosystem, GitState, Project, UpdateSeverity};
use appchecker::persist::Store;
use chrono::Utc;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn store_creates_missing_parent_directories() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-store-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let db_path = root.join("nested/data/appchecker.db");

    let store = Store::open(&db_path).expect("store should create its parent directories");
    assert!(db_path.exists());

    store.shutdown();
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn store_returns_all_dependencies_and_removes_them_with_the_project() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-store-deps-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let store = Store::open(&root.join("appchecker.db")).unwrap();
    let project = Project::new("demo".into(), PathBuf::from("/tmp/demo"));
    store.upsert_project(&project).unwrap();
    let dependencies = vec![
        DepInfo {
            name: "current".into(),
            ecosystem: Ecosystem::Cargo,
            current_version: "1.0.0".into(),
            latest_version: Some("1.0.0".into()),
            update_available: false,
            severity: UpdateSeverity::Info,
            dev_dependency: false,
            manifest_name: None,
            checked_at: Utc::now(),
        },
        DepInfo {
            name: "outdated".into(),
            ecosystem: Ecosystem::Npm,
            current_version: "1.0.0".into(),
            latest_version: Some("2.0.0".into()),
            update_available: true,
            severity: UpdateSeverity::Major,
            dev_dependency: true,
            manifest_name: None,
            checked_at: Utc::now(),
        },
    ];
    store.save_deps(project.id, &dependencies).unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let stored = store.get_deps(project.id).unwrap();
        if stored.len() == 2 {
            assert_eq!(stored[0].name, "outdated");
            assert!(stored[0].dev_dependency);
            assert_eq!(stored[1].name, "current");
            break;
        }
        assert!(Instant::now() < deadline, "dependencies were not persisted");
        std::thread::sleep(Duration::from_millis(10));
    }

    store.save_deps(project.id, &dependencies[1..2]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let stored = store.get_deps(project.id).unwrap();
        if stored.len() == 1 {
            assert_eq!(stored[0].name, "outdated");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "stale dependency was not removed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    store.remove_project(project.id).unwrap();
    store.shutdown();
    assert!(store.get_deps(project.id).unwrap().is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn dropping_a_store_clone_does_not_stop_the_writer() {
    let root =
        std::env::temp_dir().join(format!("appchecker-store-clone-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&root.join("appchecker.db")).unwrap();
    let clone = store.clone();
    drop(clone);
    let project = Project::new("demo".into(), PathBuf::from("/tmp/demo"));
    store.upsert_project(&project).unwrap();
    store
        .save_git_state(
            project.id,
            &GitState {
                scanned_at: Utc::now(),
                ..GitState::default()
            },
        )
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if store.get_git_state(project.id).unwrap().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "writer stopped after a clone was dropped"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    store.shutdown();
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn upsert_supports_path_edits_and_replaces_stale_project_ids() {
    let root = std::env::temp_dir().join(format!("appchecker-store-path-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&root.join("appchecker.db")).unwrap();
    let original = Project::new("demo".into(), PathBuf::from("/tmp/old"));
    store.upsert_project(&original).unwrap();
    let mut edited = original.clone();
    edited.path = PathBuf::from("/tmp/new");
    store.upsert_project(&edited).unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let projects = store.list_projects().unwrap();
        if projects.len() == 1 && projects[0].path == edited.path {
            assert_eq!(projects[0].id, original.id);
            break;
        }
        assert!(Instant::now() < deadline, "path edit failed: {projects:?}");
        std::thread::sleep(Duration::from_millis(10));
    }

    let replacement = Project::new("replacement".into(), edited.path.clone());
    store.upsert_project(&replacement).unwrap();
    store
        .save_git_state(
            replacement.id,
            &GitState {
                scanned_at: Utc::now(),
                ..GitState::default()
            },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let projects = store.list_projects().unwrap();
        if projects.len() == 1 && projects[0].id == replacement.id {
            assert!(store.get_git_state(replacement.id).unwrap().is_some());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "stale project id was not replaced"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    store.shutdown();
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn store_preserves_normal_dev_and_aliased_cargo_dependencies() {
    let root = std::env::temp_dir().join(format!("appchecker-store-keys-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&root.join("appchecker.db")).unwrap();
    let project = Project::new("demo".into(), PathBuf::from("/tmp/demo"));
    store.upsert_project(&project).unwrap();
    let make_dependency = |dev: bool, alias: Option<&str>| DepInfo {
        name: "serde".into(),
        ecosystem: Ecosystem::Cargo,
        current_version: "1.0.0".into(),
        latest_version: Some("2.0.0".into()),
        update_available: true,
        severity: UpdateSeverity::Major,
        dev_dependency: dev,
        manifest_name: alias.map(str::to_string),
        checked_at: Utc::now(),
    };
    let dependencies = vec![
        make_dependency(false, None),
        make_dependency(true, None),
        make_dependency(false, Some("serde_one")),
        make_dependency(false, Some("serde_two")),
    ];
    store.save_deps(project.id, &dependencies).unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let stored = store.get_deps(project.id).unwrap();
        if stored.len() == 4 {
            assert!(
                stored
                    .iter()
                    .any(|dependency| !dependency.dev_dependency
                        && dependency.manifest_name.is_none())
            );
            assert!(stored.iter().any(|dependency| dependency.dev_dependency));
            assert!(
                stored
                    .iter()
                    .any(|dependency| dependency.manifest_name.as_deref() == Some("serde_one"))
            );
            assert!(
                stored
                    .iter()
                    .any(|dependency| dependency.manifest_name.as_deref() == Some("serde_two"))
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "dependency rows collided: {stored:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    store.shutdown();
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn migration_preserves_distinct_dependencies_from_the_legacy_schema() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-store-migration-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let db_path = root.join("appchecker.db");
    let project = Project::new("demo".into(), PathBuf::from("/tmp/demo"));
    {
        let connection = rusqlite::Connection::open(&db_path).unwrap();
        connection.execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, path TEXT NOT NULL UNIQUE, tags TEXT NOT NULL DEFAULT '[]', pinned INTEGER NOT NULL DEFAULT 0, notification_overrides TEXT, added_at TEXT NOT NULL, last_scanned_at TEXT);
             CREATE TABLE deps (id INTEGER PRIMARY KEY AUTOINCREMENT, project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE, name TEXT NOT NULL, ecosystem TEXT NOT NULL, current_version TEXT NOT NULL, latest_version TEXT, update_available INTEGER NOT NULL DEFAULT 0, severity TEXT NOT NULL DEFAULT 'info', checked_at TEXT NOT NULL, dev_dependency INTEGER NOT NULL DEFAULT 0, manifest_name TEXT, UNIQUE(project_id, name, ecosystem));",
        ).unwrap();
        connection.execute(
            "INSERT INTO projects (id, name, path, tags, pinned, added_at) VALUES (?1, ?2, ?3, '[]', 0, ?4)",
            rusqlite::params![project.id.to_string(), project.name, project.path.to_string_lossy(), project.added_at.to_rfc3339()],
        ).unwrap();
    }
    let store = Store::open(&db_path).unwrap();
    let dependencies = vec![
        DepInfo {
            name: "serde".into(),
            ecosystem: Ecosystem::Cargo,
            current_version: "1.0.0".into(),
            latest_version: None,
            update_available: false,
            severity: UpdateSeverity::Info,
            dev_dependency: false,
            manifest_name: None,
            checked_at: Utc::now(),
        },
        DepInfo {
            name: "serde".into(),
            ecosystem: Ecosystem::Cargo,
            current_version: "1.0.0".into(),
            latest_version: None,
            update_available: false,
            severity: UpdateSeverity::Info,
            dev_dependency: true,
            manifest_name: None,
            checked_at: Utc::now(),
        },
        DepInfo {
            name: "serde".into(),
            ecosystem: Ecosystem::Cargo,
            current_version: "1.0.0".into(),
            latest_version: None,
            update_available: false,
            severity: UpdateSeverity::Info,
            dev_dependency: false,
            manifest_name: Some("serde_alias".into()),
            checked_at: Utc::now(),
        },
    ];
    store.save_deps(project.id, &dependencies).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let stored = store.get_deps(project.id).unwrap();
        if stored.len() == 3 {
            assert!(
                stored
                    .iter()
                    .any(|dependency| dependency.manifest_name.is_none())
            );
            assert!(
                stored
                    .iter()
                    .any(|dependency| dependency.manifest_name.as_deref() == Some("serde_alias"))
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "migration lost dependency identities: {stored:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    store.shutdown();
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn migration_discards_orphaned_legacy_cache_rows() {
    let root =
        std::env::temp_dir().join(format!("appchecker-store-orphans-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let db_path = root.join("appchecker.db");
    {
        let connection = rusqlite::Connection::open(&db_path).unwrap();
        connection.execute_batch(
            "PRAGMA foreign_keys=OFF;
             CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, path TEXT NOT NULL UNIQUE, tags TEXT NOT NULL DEFAULT '[]', pinned INTEGER NOT NULL DEFAULT 0, notification_overrides TEXT, added_at TEXT NOT NULL, last_scanned_at TEXT);
             CREATE TABLE git_state (project_id TEXT PRIMARY KEY, current_branch TEXT, head_oid TEXT, is_dirty INTEGER NOT NULL DEFAULT 0, untracked_count INTEGER NOT NULL DEFAULT 0, staged_count INTEGER NOT NULL DEFAULT 0, ahead INTEGER NOT NULL DEFAULT 0, behind INTEGER NOT NULL DEFAULT 0, has_stashes INTEGER NOT NULL DEFAULT 0, stash_count INTEGER NOT NULL DEFAULT 0, last_fetch_at TEXT, scanned_at TEXT NOT NULL);
             CREATE TABLE deps (id INTEGER PRIMARY KEY AUTOINCREMENT, project_id TEXT NOT NULL, name TEXT NOT NULL, ecosystem TEXT NOT NULL, current_version TEXT NOT NULL, latest_version TEXT, update_available INTEGER NOT NULL DEFAULT 0, severity TEXT NOT NULL DEFAULT 'info', checked_at TEXT NOT NULL, dev_dependency INTEGER NOT NULL DEFAULT 0, manifest_name TEXT, UNIQUE(project_id, name, ecosystem));
             INSERT INTO git_state (project_id, scanned_at) VALUES ('orphan', '2026-01-01T00:00:00Z');
             INSERT INTO deps (project_id, name, ecosystem, current_version, checked_at) VALUES ('orphan', 'private', 'cargo', '1.0.0', '2026-01-01T00:00:00Z');",
        ).unwrap();
    }

    let store = Store::open(&db_path).expect("legacy orphans must not brick startup");
    store.shutdown();
    let connection = rusqlite::Connection::open(&db_path).unwrap();
    let deps: i64 = connection
        .query_row("SELECT COUNT(*) FROM deps", [], |row| row.get(0))
        .unwrap();
    let git: i64 = connection
        .query_row("SELECT COUNT(*) FROM git_state", [], |row| row.get(0))
        .unwrap();
    assert_eq!(deps, 0);
    assert_eq!(git, 0);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn flush_waits_for_pending_project_writes() {
    let root =
        std::env::temp_dir().join(format!("appchecker-store-flush-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&root.join("appchecker.db")).unwrap();
    let project = Project::new("demo".into(), PathBuf::from("/tmp/demo"));
    store.upsert_project(&project).unwrap();

    store.flush().unwrap();

    assert_eq!(store.list_projects().unwrap().len(), 1);
    store.shutdown();
    std::fs::remove_dir_all(root).ok();
}

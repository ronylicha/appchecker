use appchecker::config::{ProjectDraft, ProjectManager};
use appchecker::core::{AppConfig, DepInfo, Ecosystem, UpdateSeverity};
use appchecker::desktop::DesktopApp;
use appchecker::persist::Store;
use chrono::Utc;
use std::path::Path;
use std::process::Command;

fn init_repository(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "tests@appchecker.local"],
        vec!["config", "user.name", "AppChecker tests"],
    ] {
        let status = Command::new("git")
            .args(args)
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
    }
}

fn commit_all(path: &Path) {
    for args in [vec!["add", "."], vec!["commit", "-qm", "Initial fixture"]] {
        let status = Command::new("git")
            .args(args)
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
    }
}

#[test]
fn project_update_executes_the_manifest_update() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-desktop-update-{}",
        uuid::Uuid::new_v4()
    ));
    let repository = root.join("repo");
    init_repository(&repository);
    std::fs::write(repository.join("requirements.txt"), "requests==1.0.0\n").unwrap();
    let mut config = AppConfig {
        air_gap_mode: true,
        ..AppConfig::default()
    };
    let project = ProjectManager::add(
        &mut config,
        ProjectDraft {
            path: repository.clone(),
            name: "Demo".into(),
            tags: vec![],
            pinned: false,
        },
    )
    .unwrap();
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    store.upsert_project(&project).unwrap();
    store
        .save_deps(
            project.id,
            &[DepInfo {
                name: "requests".into(),
                ecosystem: Ecosystem::Pip,
                current_version: "1.0.0".into(),
                latest_version: Some("2.0.0".into()),
                update_available: true,
                severity: UpdateSeverity::Major,
                dev_dependency: false,
                manifest_name: None,
                checked_at: Utc::now(),
            }],
        )
        .unwrap();
    store.flush().unwrap();
    commit_all(&repository);
    let mut app = DesktopApp::new(config, root.join("config.toml"), store);

    app.update_project(project.id);
    app.start_pending_updates();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while app.is_updating() {
        app.process_update_events();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let content = std::fs::read_to_string(repository.join("requirements.txt")).unwrap();
    assert_eq!(content, "requests==2.0.0\n");
    assert!(root.join("backups").is_dir());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn selective_update_only_changes_the_selected_dependency() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-desktop-selective-{}",
        uuid::Uuid::new_v4()
    ));
    let repository = root.join("repo");
    init_repository(&repository);
    std::fs::write(
        repository.join("requirements.txt"),
        "requests==1.0.0\nflask==1.0.0\n",
    )
    .unwrap();
    let mut config = AppConfig {
        air_gap_mode: true,
        ..AppConfig::default()
    };
    let project = ProjectManager::add(
        &mut config,
        ProjectDraft {
            path: repository.clone(),
            name: "Demo".into(),
            tags: vec![],
            pinned: false,
        },
    )
    .unwrap();
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    store.upsert_project(&project).unwrap();
    let dependency = |name: &str| DepInfo {
        name: name.into(),
        ecosystem: Ecosystem::Pip,
        current_version: "1.0.0".into(),
        latest_version: Some("2.0.0".into()),
        update_available: true,
        severity: UpdateSeverity::Major,
        dev_dependency: false,
        manifest_name: None,
        checked_at: Utc::now(),
    };
    store
        .save_deps(project.id, &[dependency("requests"), dependency("flask")])
        .unwrap();
    store.flush().unwrap();
    commit_all(&repository);
    let mut app = DesktopApp::new(config, root.join("config.toml"), store);

    app.update_dependency(project.id, 1);
    app.start_pending_updates();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while app.is_updating() {
        app.process_update_events();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let content = std::fs::read_to_string(repository.join("requirements.txt")).unwrap();
    assert_eq!(content, "requests==2.0.0\nflask==1.0.0\n");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn global_update_continues_after_a_project_failure() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-desktop-global-{}",
        uuid::Uuid::new_v4()
    ));
    let valid = root.join("valid");
    let invalid = root.join("invalid");
    init_repository(&valid);
    init_repository(&invalid);
    std::fs::write(valid.join("requirements.txt"), "requests==1.0.0\n").unwrap();
    std::fs::write(
        invalid.join("requirements.txt"),
        "requests[security]>=1.0.0\n",
    )
    .unwrap();
    let mut config = AppConfig {
        air_gap_mode: true,
        ..AppConfig::default()
    };
    let first = ProjectManager::add(
        &mut config,
        ProjectDraft {
            path: valid.clone(),
            name: "Valid".into(),
            tags: vec![],
            pinned: false,
        },
    )
    .unwrap();
    let second = ProjectManager::add(
        &mut config,
        ProjectDraft {
            path: invalid.clone(),
            name: "Invalid".into(),
            tags: vec![],
            pinned: false,
        },
    )
    .unwrap();
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    for project in [&first, &second] {
        store.upsert_project(project).unwrap();
        store
            .save_deps(
                project.id,
                &[DepInfo {
                    name: "requests".into(),
                    ecosystem: Ecosystem::Pip,
                    current_version: "1.0.0".into(),
                    latest_version: Some("2.0.0".into()),
                    update_available: true,
                    severity: UpdateSeverity::Major,
                    dev_dependency: false,
                    manifest_name: None,
                    checked_at: Utc::now(),
                }],
            )
            .unwrap();
    }
    store.flush().unwrap();
    commit_all(&valid);
    commit_all(&invalid);
    let mut app = DesktopApp::new(config, root.join("config.toml"), store);

    app.update_all_projects();
    app.start_pending_updates();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while app.is_updating() {
        app.process_update_events();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert_eq!(
        std::fs::read_to_string(valid.join("requirements.txt")).unwrap(),
        "requests==2.0.0\n"
    );
    assert_eq!(
        std::fs::read_to_string(invalid.join("requirements.txt")).unwrap(),
        "requests[security]>=1.0.0\n"
    );
    std::fs::remove_dir_all(root).ok();
}

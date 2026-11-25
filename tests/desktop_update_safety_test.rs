use appchecker::config::{ProjectDraft, ProjectManager};
use appchecker::core::{AppConfig, DepInfo, Ecosystem, UpdateSeverity};
use appchecker::desktop::DesktopApp;
use appchecker::persist::Store;
use chrono::Utc;
use std::path::Path;
use std::process::Command;

fn git(path: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(path)
        .status()
        .unwrap();
    assert!(status.success());
}

fn fixture() -> (
    std::path::PathBuf,
    std::path::PathBuf,
    DesktopApp,
    appchecker::core::Project,
) {
    let root = std::env::temp_dir().join(format!(
        "appchecker-desktop-safety-{}",
        uuid::Uuid::new_v4()
    ));
    let repository = root.join("repo");
    std::fs::create_dir_all(&repository).unwrap();
    git(&repository, &["init", "-q"]);
    git(
        &repository,
        &["config", "user.email", "tests@appchecker.local"],
    );
    git(&repository, &["config", "user.name", "AppChecker tests"]);
    std::fs::write(repository.join("requirements.txt"), "requests==1.0.0\n").unwrap();
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-qm", "Initial fixture"]);
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
    let app = DesktopApp::new(config, root.join("config.toml"), store);
    (root, repository, app, project)
}

fn finish(app: &mut DesktopApp) {
    app.start_pending_updates();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while app.is_updating() {
        app.process_update_events();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn dirty_worktree_is_not_modified() {
    let (root, repository, mut app, project) = fixture();
    app.update_project(project.id);
    std::fs::write(repository.join("requirements.txt"), "requests==1.5.0\n").unwrap();

    finish(&mut app);

    assert_eq!(
        std::fs::read_to_string(repository.join("requirements.txt")).unwrap(),
        "requests==1.5.0\n"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn stale_plan_is_not_applied_over_a_newer_manifest() {
    let (root, repository, mut app, project) = fixture();
    app.update_project(project.id);
    std::fs::write(repository.join("requirements.txt"), "requests==1.5.0\n").unwrap();
    git(&repository, &["add", "."]);
    git(
        &repository,
        &["commit", "-qm", "User upgraded independently"],
    );

    finish(&mut app);

    assert_eq!(
        std::fs::read_to_string(repository.join("requirements.txt")).unwrap(),
        "requests==1.5.0\n"
    );
    std::fs::remove_dir_all(root).ok();
}

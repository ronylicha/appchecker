use appchecker::core::{AppConfig, DepInfo, Ecosystem, GitState, Project, UpdateSeverity};
use appchecker::persist::Store;
use appchecker::tui::{TuiApp, UserAction, ui};
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use std::path::PathBuf;

fn dependency(name: &str, current: &str, latest: &str) -> DepInfo {
    DepInfo {
        name: name.into(),
        ecosystem: Ecosystem::Pip,
        current_version: current.into(),
        latest_version: Some(latest.into()),
        update_available: false,
        severity: UpdateSeverity::Info,
        dev_dependency: false,
        manifest_name: None,
        checked_at: Utc::now(),
    }
}

fn fixture(two_projects: bool, air_gap: bool) -> (PathBuf, TuiApp) {
    let root = std::env::temp_dir().join(format!("appchecker-layout-{}", uuid::Uuid::new_v4()));
    let first = Project::new("alpha".into(), root.join("alpha"));
    std::fs::create_dir_all(first.path.join(".git")).unwrap();
    std::fs::write(first.path.join("requirements.txt"), "requests==1.0.0\n").unwrap();
    let second = Project::new("beta".into(), root.join("beta"));
    if two_projects {
        std::fs::create_dir_all(second.path.join(".git")).unwrap();
    }
    let config = AppConfig {
        projects: if two_projects {
            vec![first.clone(), second.clone()]
        } else {
            vec![first.clone()]
        },
        air_gap_mode: air_gap,
        ..AppConfig::default()
    };
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    store.upsert_project(&first).unwrap();
    store.upsert_project(&second).unwrap();
    store
        .save_git_state(
            first.id,
            &GitState {
                current_branch: Some("main".into()),
                scanned_at: Utc::now(),
                ..GitState::default()
            },
        )
        .unwrap();
    store
        .save_git_state(
            second.id,
            &GitState {
                current_branch: Some("develop".into()),
                scanned_at: Utc::now(),
                ..GitState::default()
            },
        )
        .unwrap();
    store
        .save_deps(
            first.id,
            &[
                dependency("requests", "1.0.0", "1.0.0"),
                dependency("flask", "1.0.0", "1.0.0"),
            ],
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(40));
    let app = TuiApp::new(config, root.join("config.toml"), store);
    (root, app)
}

#[test]
fn renders_branch_details_for_every_visible_project() {
    let (root, app) = fixture(true, false);
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    let mut regions = Default::default();
    terminal
        .draw(|frame| regions = ui::draw(frame, &app))
        .unwrap();
    let contents = format!("{:?}", terminal.backend().buffer());
    assert!(contents.contains("main"), "first branch was overwritten");
    assert!(contents.contains("develop"), "second branch is missing");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn air_gap_projects_are_not_reported_as_fully_up_to_date() {
    let (root, app) = fixture(false, true);
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    let mut regions = Default::default();
    terminal
        .draw(|frame| regions = ui::draw(frame, &app))
        .unwrap();
    let contents = format!("{:?}", terminal.backend().buffer());
    assert!(
        !contents.contains("up to date"),
        "air-gap status was too optimistic"
    );
    assert!(contents.contains("air-gap"));
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn ctrl_c_exits_even_while_a_form_is_open() {
    let (root, mut app) = fixture(false, false);
    app.handle_action(UserAction::AddProject);
    app.handle_key_event(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.is_quitting());
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn dependency_selection_is_clamped_after_a_rescan() {
    let (root, mut app) = fixture(false, true);
    app.handle_action(UserAction::SwitchView(1));
    app.handle_action(UserAction::MoveDown);
    assert_eq!(app.selected_dependency(), 1);
    app.handle_action(UserAction::RefreshProject);
    assert!(app.is_scanning());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        app.process_background_events();
        if app.selected_dependency() == 0 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "selection was not clamped: {:?}",
            app.activity()
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn activity_scroll_moves_older_and_newer_without_debt() {
    let (root, mut app) = fixture(false, false);
    for _ in 0..4 {
        app.handle_action(UserAction::PinProject);
    }
    app.handle_action(UserAction::SwitchView(2));
    assert_eq!(app.activity_scroll(), 0);
    app.handle_action(UserAction::MoveUp);
    assert_eq!(app.activity_scroll(), 1);
    app.handle_action(UserAction::MoveDown);
    assert_eq!(app.activity_scroll(), 0);
    app.handle_action(UserAction::MoveUp);
    app.handle_action(UserAction::MoveToTop);
    assert!(app.activity_scroll() > 0);
    app.handle_action(UserAction::MoveToBottom);
    assert_eq!(app.activity_scroll(), 0);
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn air_gap_does_not_hide_manifest_errors() {
    let (root, mut app) = fixture(false, true);
    std::fs::write(root.join("alpha/package.json"), "{invalid json").unwrap();
    app.handle_action(UserAction::RefreshProject);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while app.is_scanning() {
        app.process_background_events();
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    app.process_background_events();
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    terminal
        .draw(|frame| {
            ui::draw(frame, &app);
        })
        .unwrap();
    let contents = format!("{:?}", terminal.backend().buffer());
    assert!(
        contents.contains("check errors"),
        "air-gap hid the local error: {contents}"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn tui_startup_bounds_legacy_excessive_concurrency() {
    let root =
        std::env::temp_dir().join(format!("appchecker-concurrency-{}", uuid::Uuid::new_v4()));
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    let config = AppConfig {
        max_concurrent_scans: usize::MAX,
        ..AppConfig::default()
    };
    let app = TuiApp::new(config, root.join("config.toml"), store);
    drop(app);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn unresolved_ranges_are_not_reported_as_up_to_date() {
    let root =
        std::env::temp_dir().join(format!("appchecker-range-layout-{}", uuid::Uuid::new_v4()));
    let project = Project::new("ranges".into(), root.join("repo"));
    std::fs::create_dir_all(project.path.join(".git")).unwrap();
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    store.upsert_project(&project).unwrap();
    store
        .save_git_state(
            project.id,
            &GitState {
                current_branch: Some("main".into()),
                scanned_at: Utc::now(),
                ..GitState::default()
            },
        )
        .unwrap();
    store
        .save_deps(
            project.id,
            &[DepInfo {
                name: "lodash".into(),
                ecosystem: Ecosystem::Npm,
                current_version: "<4.17.0".into(),
                latest_version: Some("4.17.1".into()),
                update_available: false,
                severity: UpdateSeverity::Info,
                dev_dependency: false,
                manifest_name: None,
                checked_at: Utc::now(),
            }],
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    let config = AppConfig {
        projects: vec![project],
        ..AppConfig::default()
    };
    let app = TuiApp::new(config, root.join("config.toml"), store);
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    terminal
        .draw(|frame| {
            ui::draw(frame, &app);
        })
        .unwrap();
    let contents = format!("{:?}", terminal.backend().buffer());
    assert!(
        !contents.contains("up to date"),
        "range dependency was called healthy: {contents}"
    );
    assert!(contents.contains("range limits"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn activity_top_then_down_has_no_hidden_scroll_debt() {
    let (root, mut app) = fixture(false, false);
    for _ in 0..60 {
        app.handle_action(UserAction::PinProject);
    }
    app.handle_action(UserAction::SwitchView(2));
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    let mut regions = Default::default();
    terminal
        .draw(|frame| regions = ui::draw(frame, &app))
        .unwrap();
    app.set_regions(regions);
    app.handle_action(UserAction::MoveToTop);
    let top = app.activity_scroll();
    assert!(
        top < app.activity().len() - 1,
        "scroll debt was stored at the top"
    );
    app.handle_action(UserAction::MoveDown);
    assert_eq!(app.activity_scroll(), top.saturating_sub(1));
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn private_projects_display_a_neutral_skipped_status() {
    let (root, mut app) = fixture(false, false);
    std::fs::remove_file(root.join("alpha/requirements.txt")).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(root.join("alpha"))
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(
        root.join("alpha/.npmrc"),
        "registry=https://npm.corp.internal/\n",
    )
    .unwrap();
    std::fs::write(
        root.join("alpha/package.json"),
        r#"{"dependencies":{"@corp/internal-auth":"1.0.0"}}"#,
    )
    .unwrap();
    app.handle_action(UserAction::RefreshProject);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while app.is_scanning() {
        app.process_background_events();
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    app.process_background_events();
    app.handle_action(UserAction::SwitchView(1));
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    terminal
        .draw(|frame| {
            ui::draw(frame, &app);
        })
        .unwrap();
    let contents = format!("{:?}", terminal.backend().buffer());
    assert!(
        contents.contains("private/local"),
        "private project looked healthy: {contents}"
    );
    assert!(contents.contains("Private/local dependencies skipped"));
    assert!(!contents.contains("No supported dependency manifest found"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn actionable_updates_take_priority_over_range_limits() {
    let root =
        std::env::temp_dir().join(format!("appchecker-range-updates-{}", uuid::Uuid::new_v4()));
    let project = Project::new("mixed".into(), root.join("repo"));
    std::fs::create_dir_all(project.path.join(".git")).unwrap();
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
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
    store
        .save_deps(
            project.id,
            &[
                DepInfo {
                    name: "ranged".into(),
                    ecosystem: Ecosystem::Npm,
                    current_version: "<4.0.0".into(),
                    latest_version: Some("4.0.0".into()),
                    update_available: false,
                    severity: UpdateSeverity::Info,
                    dev_dependency: false,
                    manifest_name: None,
                    checked_at: Utc::now(),
                },
                DepInfo {
                    name: "normal".into(),
                    ecosystem: Ecosystem::Npm,
                    current_version: "1.0.0".into(),
                    latest_version: Some("2.0.0".into()),
                    update_available: true,
                    severity: UpdateSeverity::Major,
                    dev_dependency: false,
                    manifest_name: None,
                    checked_at: Utc::now(),
                },
            ],
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    let app = TuiApp::new(
        AppConfig {
            projects: vec![project],
            ..AppConfig::default()
        },
        root.join("config.toml"),
        store,
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    terminal
        .draw(|frame| {
            ui::draw(frame, &app);
        })
        .unwrap();
    let contents = format!("{:?}", terminal.backend().buffer());
    assert!(
        contents.contains("1 updates"),
        "range hid an actionable update: {contents}"
    );
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn overview_keeps_last_check_and_git_error_visible_at_eighty_columns() {
    let (root, mut app) = fixture(false, false);
    std::fs::remove_dir_all(root.join("alpha")).unwrap();
    app.handle_action(UserAction::RefreshProject);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while app.is_scanning() {
        app.process_background_events();
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    app.process_background_events();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| {
            ui::draw(frame, &app);
        })
        .unwrap();
    let contents = format!("{:?}", terminal.backend().buffer());
    assert!(
        contents.contains("Last check"),
        "last check was clipped: {contents}"
    );
    assert!(
        contents.contains("Git error:"),
        "git error was clipped: {contents}"
    );
    std::fs::remove_dir_all(root).ok();
}

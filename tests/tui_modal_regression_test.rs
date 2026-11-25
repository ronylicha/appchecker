use appchecker::config::{ProjectDraft, ProjectManager};
use appchecker::core::{AppConfig, DepInfo, Ecosystem, Project, UpdateSeverity};
use appchecker::persist::Store;
use appchecker::tui::{TuiApp, UiRegions, UserAction, modals::Modal, ui};
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};

fn project(root: &std::path::Path, name: &str) -> Project {
    let path = root.join(name);
    std::fs::create_dir_all(path.join(".git")).unwrap();
    Project::new(name.into(), path)
}

fn dependency_named(name: &str) -> DepInfo {
    DepInfo {
        name: name.into(),
        ecosystem: Ecosystem::Pip,
        current_version: "1.0.0".into(),
        latest_version: Some("2.0.0".into()),
        update_available: true,
        severity: UpdateSeverity::Major,
        dev_dependency: false,
        manifest_name: None,
        checked_at: Utc::now(),
    }
}

fn app_with_projects() -> (std::path::PathBuf, Store, TuiApp) {
    let root = std::env::temp_dir().join(format!("appchecker-modal-{}", uuid::Uuid::new_v4()));
    let mut config = AppConfig {
        air_gap_mode: true,
        ..AppConfig::default()
    };
    let first = project(&root, "demo");
    std::fs::write(
        first.path.join("requirements.txt"),
        "flask==1.0.0\nrequests==1.0.0\n",
    )
    .unwrap();
    let second = project(&root, "other");
    config.projects = vec![first.clone(), second];
    ProjectManager::add(
        &mut config,
        ProjectDraft {
            path: first.path.clone(),
            name: first.name.clone(),
            tags: vec![],
            pinned: false,
        },
    )
    .err();
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    store.upsert_project(&first).unwrap();
    store
        .save_deps(
            first.id,
            &[dependency_named("requests"), dependency_named("flask")],
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    let app = TuiApp::new(config, root.join("config.toml"), store.clone());
    (root, store, app)
}

#[tokio::test]
async fn scrolling_a_confirmation_does_not_change_the_update_project() {
    let (root, _store, mut app) = app_with_projects();
    app.handle_action(UserAction::UpdateDependency);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        app.process_background_events();
        let content = std::fs::read_to_string(root.join("demo/requirements.txt")).unwrap();
        if content.contains("flask==2.0.0") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "update was not applied to the confirmed project: {:?}",
            app.activity()
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(!root.join("other").exists() || !root.join("other/requirements.txt").exists());
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn empty_discovery_closes_without_saving_configuration() {
    let (root, _store, mut app) = app_with_projects();
    app.handle_action(UserAction::DiscoverProjects);
    std::thread::sleep(std::time::Duration::from_millis(50));
    app.process_background_events();
    assert!(matches!(app.modal(), Some(Modal::Discover { paths, .. }) if paths.is_empty()));

    app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.modal().is_none());
    assert!(!root.join("config.toml").exists());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn cancel_hitbox_matches_the_rendered_button_and_update_rows_select_correctly() {
    let (root, _store, mut app) = app_with_projects();
    app.handle_action(UserAction::ShowHelp);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    let mut regions = UiRegions::default();
    terminal
        .draw(|frame| regions = ui::draw(frame, &app))
        .unwrap();
    let cancel = regions.modal_cancel.unwrap();
    assert!(
        cancel.x < 45,
        "cancel hitbox is detached from its label: {cancel:?}"
    );
    app.set_regions(regions);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: cancel.x + 1,
        row: cancel.y,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.modal().is_none());

    app.handle_action(UserAction::UpdateAll);
    let mut regions = UiRegions::default();
    terminal
        .draw(|frame| regions = ui::draw(frame, &app))
        .unwrap();
    let second_row = regions.modal_rows[1].1;
    app.set_regions(regions);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: second_row.x + 1,
        row: second_row.y,
        modifiers: KeyModifiers::NONE,
    });
    let Some(Modal::ConfirmUpdates { selected, .. }) = app.modal() else {
        panic!("confirmation closed unexpectedly");
    };
    assert_eq!(*selected, 1);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn wrapped_commands_cannot_misalign_confirmation_rows() {
    let root = std::env::temp_dir().join(format!("appchecker-modal-wrap-{}", uuid::Uuid::new_v4()));
    let project = project(&root, "demo");
    let config = AppConfig {
        projects: vec![project.clone()],
        air_gap_mode: true,
        ..AppConfig::default()
    };
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    store.upsert_project(&project).unwrap();
    let dependencies: Vec<_> = (0..6)
        .map(|index| {
            let mut dependency = dependency_named(&format!(
                "package-with-a-very-long-name-that-wraps-the-command-{index}"
            ));
            dependency.ecosystem = Ecosystem::Npm;
            dependency
        })
        .collect();
    store.save_deps(project.id, &dependencies).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    let mut app = TuiApp::new(config, root.join("config.toml"), store);
    app.handle_action(UserAction::UpdateAll);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut regions = UiRegions::default();
    terminal
        .draw(|frame| regions = ui::draw(frame, &app))
        .unwrap();

    let buffer = terminal.backend().buffer();
    for (index, row) in &regions.modal_rows {
        let text: String = (row.x..row.x + row.width)
            .map(|x| buffer.cell((x, row.y)).unwrap().symbol())
            .collect();
        assert!(
            text.contains(&format!(
                "package-with-a-very-long-name-that-wraps-the-command-{index}"
            )),
            "row {index} is not aligned with its package: {text:?}"
        );
    }
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn long_discovery_paths_stay_aligned_with_mouse_rows() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-discovery-long-path-with-many-characters-{}",
        uuid::Uuid::new_v4()
    ));
    let discovery_root = root.join("a-very-long-nested-directory-name-for-discovery");
    for name in [
        "first-repository-with-a-long-name",
        "second-repository-with-a-long-name",
        "third-repository-with-a-long-name",
    ] {
        std::fs::create_dir_all(discovery_root.join(name).join(".git")).unwrap();
    }
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    let config = AppConfig {
        auto_discover_paths: vec![discovery_root],
        ..AppConfig::default()
    };
    let mut app = TuiApp::new(config, root.join("config.toml"), store);
    app.handle_action(UserAction::DiscoverProjects);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        app.process_background_events();
        if matches!(app.modal(), Some(Modal::Discover { .. })) {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut regions = UiRegions::default();
    terminal
        .draw(|frame| regions = ui::draw(frame, &app))
        .unwrap();
    let expected: Vec<String> = match app.modal() {
        Some(Modal::Discover { paths, .. }) => paths
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect(),
        _ => panic!("discovery modal closed"),
    };
    let buffer = terminal.backend().buffer();
    for (index, row) in &regions.modal_rows {
        let text: String = (row.x..row.x + row.width)
            .map(|x| buffer.cell((x, row.y)).unwrap().symbol())
            .collect();
        let expected = &expected[*index];
        assert!(
            text.contains(expected),
            "discovery row {index} is misaligned: {text:?}"
        );
    }
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn long_project_path_keeps_its_tail_and_cursor_visible() {
    let (root, _store, mut app) = app_with_projects();
    app.handle_action(UserAction::AddProject);
    let path = "/home/developer/Projects/a-very-long-backend-service-name-for-editing";
    for character in path.chars() {
        app.handle_key_event(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| {
            ui::draw(frame, &app);
        })
        .unwrap();
    let contents = format!("{:?}", terminal.backend().buffer());
    assert!(
        contents.contains("service-name-for-editing_"),
        "path tail or cursor was clipped: {contents}"
    );
    std::fs::remove_dir_all(root).ok();
}

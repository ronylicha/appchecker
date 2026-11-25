use appchecker::config::{ConfigLoader, ProjectDraft, ProjectManager};
use appchecker::core::{AppConfig, DepInfo, Ecosystem, UpdateSeverity};
use appchecker::persist::Store;
use appchecker::tui::{TuiApp, UiRegions, UserAction, ui};
use chrono::Utc;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn fixture(with_dependency: bool) -> (PathBuf, PathBuf, Store, TuiApp) {
    let root = std::env::temp_dir().join(format!("appchecker-workflow-{}", uuid::Uuid::new_v4()));
    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    let mut config = AppConfig {
        air_gap_mode: true,
        ..AppConfig::default()
    };
    let project = ProjectManager::add(
        &mut config,
        ProjectDraft {
            path: repo.clone(),
            name: "Demo".into(),
            tags: vec![],
            pinned: false,
        },
    )
    .unwrap();
    ConfigLoader::save(&config, &root.join("config.toml")).unwrap();
    store.upsert_project(&project).unwrap();
    if with_dependency {
        std::fs::write(repo.join("requirements.txt"), "requests==1.0.0\n").unwrap();
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
        std::thread::sleep(Duration::from_millis(30));
    }
    let app = TuiApp::new(config, root.join("config.toml"), store.clone());
    (root, repo, store, app)
}

#[tokio::test]
async fn complete_project_management_is_persisted_from_the_interface() {
    let (root, repo, _store, mut app) = fixture(false);
    let second_repo = root.join("second");
    std::fs::create_dir_all(second_repo.join(".git")).unwrap();

    app.handle_action(UserAction::AddProject);
    for character in second_repo.to_string_lossy().chars() {
        app.handle_key_event(key(KeyCode::Char(character)));
    }
    app.handle_key_event(key(KeyCode::Tab));
    for character in "Second".chars() {
        app.handle_key_event(key(KeyCode::Char(character)));
    }
    app.handle_key_event(key(KeyCode::Enter));
    let config = ConfigLoader::load(&root.join("config.toml")).unwrap();
    assert_eq!(config.projects.len(), 2);
    assert!(
        config
            .projects
            .iter()
            .any(|project| project.name == "Second")
    );

    app.handle_action(UserAction::EditProject);
    app.handle_key_event(key(KeyCode::Tab));
    for _ in 0..6 {
        app.handle_key_event(key(KeyCode::Backspace));
    }
    for character in "Edited".chars() {
        app.handle_key_event(key(KeyCode::Char(character)));
    }
    app.handle_key_event(key(KeyCode::Enter));
    let config = ConfigLoader::load(&root.join("config.toml")).unwrap();
    assert!(
        config
            .projects
            .iter()
            .any(|project| project.name == "Edited")
    );

    app.handle_action(UserAction::PinProject);
    let config = ConfigLoader::load(&root.join("config.toml")).unwrap();
    assert!(
        config
            .projects
            .iter()
            .find(|project| project.name == "Edited")
            .unwrap()
            .pinned
    );

    app.handle_action(UserAction::RemoveProject);
    app.handle_key_event(key(KeyCode::Enter));
    let config = ConfigLoader::load(&root.join("config.toml")).unwrap();
    assert_eq!(config.projects.len(), 1);
    assert_eq!(config.projects[0].path, repo);
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn updates_do_not_race_an_active_scan() {
    let (root, _repo, _store, mut app) = fixture(true);
    app.handle_action(UserAction::RefreshProject);
    app.handle_action(UserAction::UpdateDependency);

    assert!(app.modal().is_none());
    assert_eq!(
        std::fs::read_to_string(root.join("repo/requirements.txt")).unwrap(),
        "requests==1.0.0\n"
    );
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn update_confirmation_executes_the_manifest_update_and_refreshes_state() {
    let (root, repo, _store, mut app) = fixture(true);
    app.handle_action(UserAction::UpdateDependency);
    app.set_regions(UiRegions {
        modal_confirm: Some(Rect::new(0, 0, 20, 1)),
        ..UiRegions::default()
    });
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 1,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        app.process_background_events();
        let content = std::fs::read_to_string(repo.join("requirements.txt")).unwrap();
        if content.contains("requests==2.0.0") {
            break;
        }
        assert!(Instant::now() < deadline, "update was not executed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    app.process_background_events();
    assert!(
        std::fs::read_to_string(repo.join("requirements.txt"))
            .unwrap()
            .contains("requests==2.0.0")
    );
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn path_edit_invalidates_the_previous_scan_result() {
    let (root, _repo, store, mut app) = fixture(true);
    app.handle_action(UserAction::RefreshProject);
    let new_repo = root.join("new-repo");
    std::fs::create_dir_all(new_repo.join(".git")).unwrap();
    std::fs::write(new_repo.join("requirements.txt"), "flask==1.0.0\n").unwrap();

    app.handle_action(UserAction::EditProject);
    let old_path = root.join("repo").to_string_lossy().len();
    for _ in 0..old_path {
        app.handle_key_event(key(KeyCode::Backspace));
    }
    for character in new_repo.to_string_lossy().chars() {
        app.handle_key_event(key(KeyCode::Char(character)));
    }
    app.handle_key_event(key(KeyCode::Enter));

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        app.process_background_events();
        let projects = store.list_projects().unwrap();
        if projects.len() == 1 && projects[0].path == new_repo {
            let dependencies = store.get_deps(projects[0].id).unwrap();
            if !dependencies.is_empty() {
                assert!(
                    dependencies
                        .iter()
                        .any(|dependency| dependency.name == "flask")
                );
                assert!(
                    !dependencies
                        .iter()
                        .any(|dependency| dependency.name == "requests")
                );
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "stale scan result was not discarded"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn discovery_completion_does_not_clobber_an_open_form() {
    let (root, _repo, _store, mut app) = fixture(false);
    app.handle_action(UserAction::DiscoverProjects);
    app.handle_action(UserAction::AddProject);

    tokio::time::sleep(Duration::from_millis(50)).await;
    app.process_background_events();

    assert!(matches!(
        app.modal(),
        Some(appchecker::tui::modals::Modal::AddProject { .. })
    ));
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn failed_config_save_preserves_cached_project_state() {
    let (root, _repo, store, mut app) = fixture(true);
    let project_id = store.list_projects().unwrap()[0].id;
    std::fs::remove_file(root.join("config.toml")).unwrap();
    std::fs::create_dir(root.join("config.toml")).unwrap();
    let new_repo = root.join("new-repo");
    std::fs::create_dir_all(new_repo.join(".git")).unwrap();

    app.handle_action(UserAction::EditProject);
    let old_path = root.join("repo").to_string_lossy().len();
    for _ in 0..old_path {
        app.handle_key_event(key(KeyCode::Backspace));
    }
    for character in new_repo.to_string_lossy().chars() {
        app.handle_key_event(key(KeyCode::Char(character)));
    }
    app.handle_key_event(key(KeyCode::Enter));

    let dependencies = store.get_deps(project_id).unwrap();
    assert!(
        !dependencies.is_empty(),
        "cache was cleared despite the failed save"
    );
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn failed_project_removal_does_not_leave_a_scan_permanently_stuck() {
    let (root, _repo, _store, mut app) = fixture(true);
    std::fs::remove_file(root.join("config.toml")).unwrap();
    std::fs::create_dir(root.join("config.toml")).unwrap();

    app.handle_action(UserAction::RefreshProject);
    app.handle_action(UserAction::RemoveProject);
    app.handle_key_event(key(KeyCode::Enter));

    let deadline = Instant::now() + Duration::from_secs(3);
    while app.is_scanning() {
        app.process_background_events();
        assert!(
            Instant::now() < deadline,
            "scan remained stuck after the aborted task could not report completion"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    app.handle_action(UserAction::RefreshProject);
    assert!(app.is_scanning(), "a new scan could not be started");
    std::fs::remove_dir_all(root).ok();
}

#[tokio::test]
async fn discovery_preserves_the_selected_project_and_resets_dependency_selection() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-discovery-selection-{}",
        uuid::Uuid::new_v4()
    ));
    let selected = appchecker::core::Project::new("zeta".into(), root.join("zeta"));
    std::fs::create_dir_all(selected.path.join(".git")).unwrap();
    std::fs::write(
        selected.path.join("requirements.txt"),
        "requests==1.0.0\nflask==1.0.0\n",
    )
    .unwrap();
    let discovered = root.join("alpha");
    std::fs::create_dir_all(discovered.join(".git")).unwrap();
    let config = AppConfig {
        projects: vec![selected.clone()],
        auto_discover_paths: vec![root.clone()],
        air_gap_mode: true,
        ..AppConfig::default()
    };
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    store.upsert_project(&selected).unwrap();
    store
        .save_deps(
            selected.id,
            &[
                DepInfo {
                    name: "requests".into(),
                    ecosystem: Ecosystem::Pip,
                    current_version: "1.0.0".into(),
                    latest_version: Some("2.0.0".into()),
                    update_available: true,
                    severity: UpdateSeverity::Major,
                    dev_dependency: false,
                    manifest_name: None,
                    checked_at: Utc::now(),
                },
                DepInfo {
                    name: "flask".into(),
                    ecosystem: Ecosystem::Pip,
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
    std::thread::sleep(Duration::from_millis(30));
    let mut app = TuiApp::new(config, root.join("config.toml"), store);
    app.handle_action(UserAction::SwitchView(1));
    app.handle_action(UserAction::MoveDown);
    assert_eq!(app.selected_dependency(), 1);

    app.handle_action(UserAction::DiscoverProjects);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        app.process_background_events();
        if app.modal().is_some() {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    app.handle_key_event(key(KeyCode::Enter));

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| {
            ui::draw(frame, &app);
        })
        .unwrap();
    let contents = format!("{:?}", terminal.backend().buffer());
    assert!(
        contents.contains("2 total"),
        "selection jumped to the new project: {contents}"
    );
    assert!(
        contents.contains("requests"),
        "previous dependencies were not selected"
    );
    assert_eq!(app.selected_dependency(), 0);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn project_home_and_end_reset_dependency_selection() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-selection-reset-{}",
        uuid::Uuid::new_v4()
    ));
    let first = appchecker::core::Project::new("first".into(), root.join("first"));
    let second = appchecker::core::Project::new("second".into(), root.join("second"));
    std::fs::create_dir_all(first.path.join(".git")).unwrap();
    std::fs::create_dir_all(second.path.join(".git")).unwrap();
    let config = AppConfig {
        projects: vec![first.clone(), second.clone()],
        air_gap_mode: true,
        ..AppConfig::default()
    };
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    store.upsert_project(&first).unwrap();
    store.upsert_project(&second).unwrap();
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
        .save_deps(
            first.id,
            &[dependency("one"), dependency("two"), dependency("three")],
        )
        .unwrap();
    store.save_deps(second.id, &[dependency("only")]).unwrap();
    std::thread::sleep(Duration::from_millis(30));
    let mut app = TuiApp::new(config, root.join("config.toml"), store);
    app.handle_action(UserAction::SwitchView(1));
    app.handle_action(UserAction::MoveDown);
    app.handle_action(UserAction::MoveDown);
    assert_eq!(app.selected_dependency(), 2);
    app.handle_action(UserAction::Close);
    app.handle_action(UserAction::MoveToBottom);
    assert_eq!(app.selected_dependency(), 0);
    app.handle_action(UserAction::MoveToTop);
    assert_eq!(app.selected_dependency(), 0);
    std::fs::remove_dir_all(root).ok();
}

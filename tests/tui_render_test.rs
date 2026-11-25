use appchecker::config::{ProjectDraft, ProjectManager};
use appchecker::core::{AppConfig, DepInfo, Ecosystem, UpdateSeverity};
use appchecker::persist::Store;
use appchecker::tui::{TuiApp, View, ui};
use chrono::Utc;
use ratatui::{Terminal, backend::TestBackend};

fn test_app() -> (TuiApp, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("appchecker-tui-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("repo/.git")).unwrap();
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    let mut config = AppConfig::default();
    let project = ProjectManager::add(
        &mut config,
        ProjectDraft {
            path: root.join("repo"),
            name: "Demo".into(),
            tags: vec!["test".into()],
            pinned: true,
        },
    )
    .unwrap();
    store.upsert_project(&project).unwrap();
    store
        .save_deps(
            project.id,
            &[DepInfo {
                name: "serde".into(),
                ecosystem: Ecosystem::Cargo,
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
    std::thread::sleep(std::time::Duration::from_millis(30));
    let app = TuiApp::new(config, root.join("config.toml"), store);
    (app, root)
}

#[test]
fn renders_real_projects_dependencies_and_mouse_targets() {
    let (mut app, root) = test_app();
    let backend = TestBackend::new(120, 36);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut regions = Default::default();
    terminal
        .draw(|frame| regions = ui::draw(frame, &app))
        .unwrap();

    let contents = format!("{:?}", terminal.backend().buffer());
    assert!(contents.contains("Projects"));
    assert!(contents.contains("Dependencies"));
    assert!(contents.contains("Activity"));
    assert!(contents.contains("Demo"));
    assert!(contents.contains("serde"));
    assert!(contents.contains("2.0.0"));
    assert!(!regions.project_rows.is_empty());
    assert!(!regions.dependency_rows.is_empty());

    app.set_regions(regions);
    let tab = app.regions().tabs[1].1;
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: tab.x + 1,
        row: tab.y,
        modifiers: crossterm::event::KeyModifiers::NONE,
    });
    assert_eq!(app.view(), View::Dependencies);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn all_action_buttons_remain_clickable_at_eighty_columns() {
    let (app, root) = test_app();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut regions = Default::default();
    terminal
        .draw(|frame| regions = ui::draw(frame, &app))
        .unwrap();

    for action in [
        appchecker::tui::ui::ButtonAction::Refresh,
        appchecker::tui::ui::ButtonAction::RefreshAll,
        appchecker::tui::ui::ButtonAction::UpdateAll,
        appchecker::tui::ui::ButtonAction::Config,
        appchecker::tui::ui::ButtonAction::Help,
    ] {
        assert!(
            regions
                .buttons
                .iter()
                .any(|(candidate, area)| { *candidate == action && area.x + area.width <= 80 }),
            "action is not clickable at 80 columns: {action:?}"
        );
    }
    std::fs::remove_dir_all(root).ok();
}

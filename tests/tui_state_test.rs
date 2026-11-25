use appchecker::config::ProjectManager;
use appchecker::core::AppConfig;
use appchecker::tui::{ConfigEvent, ConfigForm, FormEvent, ProjectForm, UserAction, parse_key};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn project_form_edits_fields_and_submits_a_normalized_draft() {
    let mut form = ProjectForm::new();
    for character in "/tmp/demo".chars() {
        form.handle_key(key(KeyCode::Char(character)));
    }
    form.handle_key(key(KeyCode::Tab));
    for character in "Demo".chars() {
        form.handle_key(key(KeyCode::Char(character)));
    }
    form.handle_key(key(KeyCode::Tab));
    for character in "rust, cli".chars() {
        form.handle_key(key(KeyCode::Char(character)));
    }
    form.handle_key(key(KeyCode::Tab));
    form.handle_key(key(KeyCode::Char(' ')));

    let event = form.handle_key(key(KeyCode::Enter));
    let FormEvent::Submit(draft) = event else {
        panic!("form did not submit");
    };
    assert_eq!(draft.path, PathBuf::from("/tmp/demo"));
    assert_eq!(draft.name, "Demo");
    assert_eq!(draft.tags, vec!["rust", "cli"]);
    assert!(draft.pinned);
}

#[test]
fn project_form_cancels_without_creating_a_draft() {
    let mut form = ProjectForm::new();
    assert_eq!(form.handle_key(key(KeyCode::Esc)), FormEvent::Cancel);
}

#[test]
fn edit_form_preserves_the_current_project_values() {
    let mut config = AppConfig::default();
    let root = std::env::temp_dir().join(format!("appchecker-form-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join(".git")).unwrap();
    let project = ProjectManager::add(
        &mut config,
        appchecker::config::ProjectDraft {
            path: root.clone(),
            name: "Current".into(),
            tags: vec!["one".into(), "two".into()],
            pinned: true,
        },
    )
    .unwrap();

    let form = ProjectForm::from_project(&project);

    assert_eq!(form.path, root.to_string_lossy());
    assert_eq!(form.name, "Current");
    assert_eq!(form.tags, "one, two");
    assert!(form.pinned);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn config_form_validates_numbers_and_toggles_air_gap() {
    let mut form = ConfigForm::new(300, 8, false);
    form.handle_key(key(KeyCode::Tab));
    form.handle_key(key(KeyCode::Tab));
    form.handle_key(key(KeyCode::Char(' ')));
    let ConfigEvent::Submit {
        scan_interval,
        max_concurrent,
        air_gap,
    } = form.handle_key(key(KeyCode::Enter))
    else {
        panic!("config did not submit");
    };
    assert_eq!(scan_interval, 300);
    assert_eq!(max_concurrent, 8);
    assert!(air_gap);

    let mut invalid = ConfigForm::new(0, 8, false);
    assert_eq!(
        invalid.handle_key(key(KeyCode::Enter)),
        ConfigEvent::Error("Scan interval must be greater than zero".into())
    );

    let mut excessive = ConfigForm::new(300, 3_000_000_000_000_000_000, false);
    assert_eq!(
        excessive.handle_key(key(KeyCode::Enter)),
        ConfigEvent::Error("Concurrent scans must be between 1 and 256".into())
    );
}

#[test]
fn keyboard_actions_only_expose_implemented_operations() {
    assert_eq!(parse_key(key(KeyCode::Char('a'))), UserAction::AddProject);
    assert_eq!(parse_key(key(KeyCode::Char('e'))), UserAction::EditProject);
    assert_eq!(
        parse_key(key(KeyCode::Char('d'))),
        UserAction::RemoveProject
    );
    assert_eq!(
        parse_key(key(KeyCode::Char('f'))),
        UserAction::DiscoverProjects
    );
    assert_eq!(
        parse_key(key(KeyCode::Char('r'))),
        UserAction::RefreshProject
    );
    assert_eq!(parse_key(key(KeyCode::Char('R'))), UserAction::RefreshAll);
    assert_eq!(
        parse_key(key(KeyCode::Char('u'))),
        UserAction::UpdateDependency
    );
    assert_eq!(parse_key(key(KeyCode::Char('U'))), UserAction::UpdateAll);
    assert_eq!(
        parse_key(key(KeyCode::Char('2'))),
        UserAction::SwitchView(1)
    );
    assert_eq!(
        parse_key(key(KeyCode::Char('3'))),
        UserAction::SwitchView(2)
    );
    assert_eq!(parse_key(key(KeyCode::Enter)), UserAction::ShowDependencies);
}

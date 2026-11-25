use appchecker::config::ProjectDraft;
use appchecker::core::AppConfig;
use appchecker::desktop::DesktopApp;
use appchecker::persist::Store;

fn fixture() -> (std::path::PathBuf, Store, DesktopApp) {
    let root = std::env::temp_dir().join(format!(
        "appchecker-desktop-projects-{}",
        uuid::Uuid::new_v4()
    ));
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();
    let app = DesktopApp::new(
        AppConfig::default(),
        root.join("config.toml"),
        store.clone(),
    );
    (root, store, app)
}

#[test]
fn project_lifecycle_is_persisted() {
    let (root, _store, mut app) = fixture();
    let repository = root.join("repo");
    std::fs::create_dir_all(repository.join(".git")).unwrap();

    app.add_project(ProjectDraft {
        path: repository.clone(),
        name: "Demo".into(),
        tags: vec!["rust".into()],
        pinned: true,
    })
    .unwrap();
    assert_eq!(app.projects().len(), 1);
    assert!(app.projects()[0].pinned);

    app.toggle_pin(app.projects()[0].id).unwrap();
    assert!(!app.projects()[0].pinned);

    let project_id = app.projects()[0].id;
    app.set_project_trusted(project_id, true).unwrap();
    let persisted = appchecker::config::ConfigLoader::load(&root.join("config.toml")).unwrap();
    assert!(persisted.trusted_repositories.contains(&project_id));

    app.remove_project(app.projects()[0].id).unwrap();
    assert!(app.projects().is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn duplicate_and_invalid_projects_show_an_error() {
    let (root, _store, mut app) = fixture();
    let repository = root.join("repo");
    std::fs::create_dir_all(repository.join(".git")).unwrap();
    let draft = ProjectDraft {
        path: repository,
        name: "Demo".into(),
        tags: vec![],
        pinned: false,
    };
    app.add_project(draft.clone()).unwrap();
    assert!(app.add_project(draft).is_err());
    std::fs::remove_dir_all(root).ok();
}

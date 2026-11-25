use appchecker::config::{ProjectDraft, ProjectManager};
use appchecker::core::AppConfig;
use appchecker::persist::Store;
use appchecker::tui::TuiApp;

#[test]
fn startup_synchronizes_cli_projects_before_scanning() {
    let root = std::env::temp_dir().join(format!("appchecker-startup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("repo/.git")).unwrap();
    let mut config = AppConfig::default();
    let project = ProjectManager::add(
        &mut config,
        ProjectDraft {
            path: root.join("repo"),
            name: "CLI project".into(),
            tags: vec![],
            pinned: false,
        },
    )
    .unwrap();
    let store = Store::open(&root.join("data/appchecker.db")).unwrap();

    let _app = TuiApp::new(config, root.join("config.toml"), store.clone());

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let projects = store.list_projects().unwrap();
        if projects.len() == 1 {
            assert_eq!(projects[0].id, project.id);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "project was not synchronized"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    std::fs::remove_dir_all(root).ok();
}

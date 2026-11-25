use appchecker::config::{ProjectDraft, ProjectManager};
use appchecker::core::AppConfig;
use std::path::{Path, PathBuf};

struct TestRepo {
    root: PathBuf,
}

impl TestRepo {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "appchecker-project-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let path = root.join(label);
        std::fs::create_dir_all(path.join(".git")).unwrap();
        Self { root }
    }

    fn path(&self, label: &str) -> PathBuf {
        self.root.join(label)
    }
}

impl Drop for TestRepo {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

fn draft(path: &Path) -> ProjectDraft {
    ProjectDraft {
        path: path.to_path_buf(),
        name: String::new(),
        tags: vec![" rust ".into(), "cli".into(), "rust".into(), "".into()],
        pinned: false,
    }
}

#[test]
fn add_normalizes_project_and_sorts_pinned_projects_first() {
    let repos = TestRepo::new("alpha");
    let mut config = AppConfig::default();

    let alpha = ProjectManager::add(&mut config, draft(&repos.path("alpha"))).unwrap();
    let mut pinned_draft = draft(&repos.path("alpha"));
    pinned_draft.path = repos.root.join("beta");
    std::fs::create_dir_all(pinned_draft.path.join(".git")).unwrap();
    pinned_draft.name = "Beta".into();
    pinned_draft.pinned = true;
    ProjectManager::add(&mut config, pinned_draft).unwrap();

    assert_eq!(alpha.name, "alpha");
    assert_eq!(alpha.tags, vec!["rust", "cli"]);
    assert_eq!(config.projects[0].name, "Beta");
    assert_eq!(config.projects[1].name, "alpha");
}

#[test]
fn add_rejects_a_directory_without_git_metadata() {
    let repos = TestRepo::new("alpha");
    let plain = repos.path("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let mut config = AppConfig::default();

    let error = ProjectManager::add(&mut config, draft(&plain)).unwrap_err();

    assert!(error.to_string().contains("not a Git repository"));
    assert!(config.projects.is_empty());
}

#[test]
fn add_rejects_the_same_canonical_path_twice() {
    let repos = TestRepo::new("alpha");
    let mut config = AppConfig::default();
    ProjectManager::add(&mut config, draft(&repos.path("alpha"))).unwrap();

    let error = ProjectManager::add(&mut config, draft(&repos.path("alpha"))).unwrap_err();

    assert!(error.to_string().contains("already monitored"));
    assert_eq!(config.projects.len(), 1);
}

#[test]
fn update_changes_metadata_without_changing_identity() {
    let repos = TestRepo::new("alpha");
    let mut config = AppConfig::default();
    let original = ProjectManager::add(&mut config, draft(&repos.path("alpha"))).unwrap();
    let mut changes = draft(&repos.path("alpha"));
    changes.name = "Renamed".into();
    changes.tags = vec!["production".into()];
    changes.pinned = true;

    let updated = ProjectManager::update(&mut config, original.id, changes).unwrap();

    assert_eq!(updated.id, original.id);
    assert_eq!(updated.added_at, original.added_at);
    assert_eq!(updated.name, "Renamed");
    assert_eq!(updated.tags, vec!["production"]);
    assert!(updated.pinned);
}

#[test]
fn remove_deletes_only_the_requested_project() {
    let repos = TestRepo::new("alpha");
    let mut config = AppConfig::default();
    let project = ProjectManager::add(&mut config, draft(&repos.path("alpha"))).unwrap();

    let removed = ProjectManager::remove(&mut config, project.id).unwrap();

    assert_eq!(removed.id, project.id);
    assert!(config.projects.is_empty());
    assert!(ProjectManager::remove(&mut config, project.id).is_err());
}

#[test]
fn toggle_pin_changes_and_preserves_the_selection_state() {
    let repos = TestRepo::new("alpha");
    let mut config = AppConfig::default();
    let project = ProjectManager::add(&mut config, draft(&repos.path("alpha"))).unwrap();

    assert!(ProjectManager::toggle_pin(&mut config, project.id).unwrap());
    assert!(config.projects[0].pinned);
    assert!(!ProjectManager::toggle_pin(&mut config, project.id).unwrap());
    assert!(!config.projects[0].pinned);
}

#[test]
fn find_matches_legacy_non_canonical_project_paths() {
    let repos = TestRepo::new("alpha");
    let mut config = AppConfig::default();
    let project = appchecker::core::Project::new("alpha".into(), repos.path("alpha").join("."));
    config.projects.push(project.clone());

    let found = ProjectManager::find(&config, &repos.path("alpha").to_string_lossy()).unwrap();

    assert_eq!(found.id, project.id);
    std::fs::remove_dir_all(&repos.root).ok();
}

#[test]
fn discover_adds_only_repositories_not_already_monitored() {
    let repos = TestRepo::new("alpha");
    std::fs::create_dir_all(repos.path("beta/.git")).unwrap();
    let mut config = AppConfig::default();
    ProjectManager::add(&mut config, draft(&repos.path("alpha"))).unwrap();

    let added = ProjectManager::discover(&mut config, std::slice::from_ref(&repos.root), 3);

    assert_eq!(added.len(), 1);
    assert_eq!(added[0].name, "beta");
    assert_eq!(config.projects.len(), 2);
}

#[test]
fn discovery_stops_when_its_cancellation_flag_is_set() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-discovery-cancel-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(root.join("repo/.git")).unwrap();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let discovered = appchecker::config::ConfigLoader::auto_discover_with_cancel(
        std::slice::from_ref(&root),
        3,
        &[],
        cancel,
    );
    assert!(discovered.is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn discovery_accepts_git_files_for_worktrees_and_submodules() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-worktree-discovery-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join(".git"),
        "gitdir: /tmp/main-worktree/.git/worktrees/example\n",
    )
    .unwrap();
    let discovered =
        appchecker::config::ConfigLoader::auto_discover(std::slice::from_ref(&root), 1, &[]);
    assert_eq!(discovered, vec![root.clone()]);
    std::fs::remove_dir_all(root).ok();
}

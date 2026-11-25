use appchecker::config::ConfigLoader;
use appchecker::core::AppConfig;
use appchecker::persist::Store;
use std::path::PathBuf;
use std::process::Command;

fn fixture() -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!("appchecker-cli-{}", uuid::Uuid::new_v4()));
    let repository = root.join("repo");
    std::fs::create_dir_all(&repository).unwrap();
    let initialized = Command::new("git")
        .args(["init", "--quiet"])
        .arg(&repository)
        .status()
        .unwrap();
    assert!(initialized.success());
    (root, repository)
}

fn appchecker(config: &PathBuf) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_appchecker"));
    command.args(["--config"]).arg(config);
    command
}

#[test]
fn add_uses_the_same_validation_and_duplicate_rules_as_the_tui() {
    let (root, repository) = fixture();
    let config = root.join("config.toml");

    let added = appchecker(&config)
        .args(["add"])
        .arg(&repository)
        .args(["--name", "Demo", "--tags", "rust,cli", "--pin"])
        .output()
        .unwrap();
    assert!(added.status.success(), "{:?}", added);
    let duplicate = appchecker(&config)
        .args(["add"])
        .arg(&repository)
        .output()
        .unwrap();
    assert!(!duplicate.status.success());
    let listed = appchecker(&config)
        .args(["list", "--detailed"])
        .output()
        .unwrap();
    assert!(listed.status.success());
    assert!(String::from_utf8_lossy(&listed.stdout).contains("Demo"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn check_reports_registry_errors_and_respects_air_gap_mode() {
    let (root, repository) = fixture();
    let config = root.join("config.toml");
    assert!(
        appchecker(&config)
            .args(["add"])
            .arg(&repository)
            .output()
            .unwrap()
            .status
            .success()
    );
    let content = std::fs::read_to_string(&config)
        .unwrap()
        .replace("air_gap_mode = false", "air_gap_mode = true");
    std::fs::write(&config, content).unwrap();

    let output = appchecker(&config)
        .args(["check", "--format", "json"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{:?}", output);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json[0]["project"], "repo");
    assert_eq!(json[0]["total_deps"], 0);
    assert_eq!(json[0]["outdated_deps"], 0);
    assert_eq!(json[0]["registry_errors"], 0);
    assert_eq!(json[0]["air_gap_mode"], true);
    assert!(json[0]["registry_error_details"].is_array());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn discover_expands_home_and_preserves_unique_projects() {
    let (root, first) = fixture();
    let second = root.join("second");
    std::fs::create_dir_all(&second).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(&second)
            .status()
            .unwrap()
            .success()
    );
    let config = root.join("config.toml");
    assert!(
        appchecker(&config)
            .args(["add"])
            .arg(&first)
            .output()
            .unwrap()
            .status
            .success()
    );

    let discovered = appchecker(&config)
        .env("HOME", &root)
        .args(["discover", "--paths", "~/repo,~/second"])
        .output()
        .unwrap();

    assert!(discovered.status.success(), "{discovered:?}");
    let listed = appchecker(&config).args(["list"]).output().unwrap();
    let output = String::from_utf8_lossy(&listed.stdout);
    assert_eq!(output.matches("repo").count(), 1, "{output}");
    assert_eq!(output.matches("second").count(), 1, "{output}");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn remove_rejects_ambiguous_names_and_removes_cache_by_path() {
    let (root, first) = fixture();
    let second = root.join("other").join("repo");
    std::fs::create_dir_all(&second).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(&second)
            .status()
            .unwrap()
            .success()
    );
    let config = root.join("config.toml");
    let data_home = root.join("data");
    for path in [&first, &second] {
        assert!(
            appchecker(&config)
                .env("XDG_DATA_HOME", &data_home)
                .args(["add"])
                .arg(path)
                .args(["--name", "repo"])
                .output()
                .unwrap()
                .status
                .success()
        );
    }

    let ambiguous = appchecker(&config)
        .env("XDG_DATA_HOME", &data_home)
        .args(["remove", "repo"])
        .output()
        .unwrap();
    assert!(!ambiguous.status.success());

    let store = Store::open(&data_home.join("appchecker/appchecker.db")).unwrap();
    let config_data = ConfigLoader::load(&config).unwrap();
    for project in &config_data.projects {
        store.upsert_project(project).unwrap();
    }
    let removed = appchecker(&config)
        .env("XDG_DATA_HOME", &data_home)
        .current_dir(&root)
        .args(["remove", "other/repo"])
        .output()
        .unwrap();
    assert!(removed.status.success(), "{removed:?}");

    let remaining = ConfigLoader::load(&config).unwrap();
    assert_eq!(remaining.projects.len(), 1);
    assert_eq!(remaining.projects[0].path, first);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let projects = store.list_projects().unwrap();
        if projects.len() == 1 {
            assert_eq!(projects[0].path, first);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "cache was not cleaned: {projects:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    store.shutdown();
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn tilde_config_path_preserves_the_user_supplied_file() {
    let root =
        std::env::temp_dir().join(format!("appchecker-config-home-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();

    let output = appchecker(&PathBuf::from("~/custom.toml"))
        .env("HOME", &root)
        .args(["config", "--reset"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert!(root.join("custom.toml").exists());
    assert!(!root.join(".config/appchecker/config.toml").exists());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn remove_works_when_the_project_directory_was_deleted() {
    let (root, first) = fixture();
    let second = root.join("other").join("repo");
    std::fs::create_dir_all(&second).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .arg(&second)
            .status()
            .unwrap()
            .success()
    );
    let config = root.join("config.toml");
    for path in [&first, &second] {
        assert!(
            appchecker(&config)
                .args(["add"])
                .arg(path)
                .args(["--name", "repo"])
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    std::fs::remove_dir_all(&second).unwrap();

    let removed = appchecker(&config)
        .args(["remove"])
        .arg(&second)
        .output()
        .unwrap();

    assert!(removed.status.success(), "{removed:?}");
    assert_eq!(ConfigLoader::load(&config).unwrap().projects.len(), 1);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn check_rejects_an_unknown_project_instead_of_returning_an_empty_success() {
    let (root, _repository) = fixture();
    let config = root.join("config.toml");

    let output = appchecker(&config)
        .args(["check", "--project", "does-not-exist"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn daemon_uses_the_configured_scan_interval() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-daemon-interval-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let config = root.join("config.toml");
    ConfigLoader::save(
        &appchecker::core::AppConfig {
            scan_interval_secs: 1,
            air_gap_mode: true,
            ..AppConfig::default()
        },
        &config,
    )
    .unwrap();
    let mut child = appchecker(&config)
        .args(["daemon", "--foreground"])
        .env("XDG_DATA_HOME", root.join("data"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(400));
    child.kill().unwrap();
    let output = child.wait_with_output().unwrap();
    let logs = String::from_utf8_lossy(&output.stderr);
    assert!(
        logs.contains("interval=1s"),
        "configured interval was ignored: {logs}"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn check_marks_private_projects_as_skipped_instead_of_healthy() {
    let (root, repository) = fixture();
    std::fs::write(
        repository.join(".npmrc"),
        "registry=https://npm.corp.internal/\n",
    )
    .unwrap();
    std::fs::write(
        repository.join("package.json"),
        r#"{"dependencies":{"@corp/internal-auth":"1.0.0"}}"#,
    )
    .unwrap();
    let config = root.join("config.toml");
    assert!(
        appchecker(&config)
            .args(["add"])
            .arg(&repository)
            .output()
            .unwrap()
            .status
            .success()
    );
    let content = std::fs::read_to_string(&config)
        .unwrap()
        .replace("air_gap_mode = false", "air_gap_mode = true");
    std::fs::write(&config, content).unwrap();

    let output = appchecker(&config)
        .args(["check", "--format", "json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json[0]["total_deps"], 0);
    assert_eq!(json[0]["non_public_skipped"], true);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn discover_uses_the_configured_depth() {
    let root = std::env::temp_dir().join(format!("appchecker-cli-depth-{}", uuid::Uuid::new_v4()));
    let nested = root.join("one/two/three/project");
    std::fs::create_dir_all(nested.join(".git")).unwrap();
    let config = root.join("config.toml");
    ConfigLoader::save(
        &AppConfig {
            auto_discover_paths: vec![root.clone()],
            max_auto_discover_depth: 5,
            ..AppConfig::default()
        },
        &config,
    )
    .unwrap();

    let output = appchecker(&config).args(["discover"]).output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Added 1 discovered projects"), "{text}");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn partial_configuration_uses_defaults_for_missing_fields() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-partial-config-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let config = root.join("config.toml");
    std::fs::write(&config, "scan_interval_secs = 60\n").unwrap();
    let loaded = ConfigLoader::load(&config).unwrap();
    assert_eq!(loaded.scan_interval_secs, 60);
    assert!(loaded.projects.is_empty());
    assert_eq!(loaded.max_concurrent_scans, 8);
    assert_eq!(loaded.file_descriptor_limit, 16384);
    assert_eq!(loaded.registry_urls.npm, "https://registry.npmjs.org");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn daemon_detaches_in_background_and_leaves_a_worker() {
    let root = std::env::temp_dir().join(format!(
        "appchecker-daemon-background-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let config = root.join("config.toml");
    ConfigLoader::save(
        &AppConfig {
            air_gap_mode: true,
            ..AppConfig::default()
        },
        &config,
    )
    .unwrap();
    let mut parent = appchecker(&config)
        .args(["daemon", "--interval", "30"])
        .env("XDG_DATA_HOME", root.join("data"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(400));
    let finished = parent.try_wait().unwrap();
    if finished.is_none() {
        parent.kill().unwrap();
        parent.wait().unwrap();
        panic!("background daemon did not return");
    }
    assert!(finished.unwrap().success());

    let processes = Command::new("ps")
        .args(["-eo", "pid,args"])
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&processes.stdout);
    let worker = listing
        .lines()
        .find(|line| {
            line.contains(config.to_string_lossy().as_ref()) && line.contains("daemon --foreground")
        })
        .expect("foreground worker was not left running");
    assert!(
        worker.contains("--interval 30"),
        "interval was dropped: {worker}"
    );
    let pid = worker.split_whitespace().next().unwrap();
    assert!(Command::new("kill").arg(pid).status().unwrap().success());
    std::fs::remove_dir_all(root).ok();
}

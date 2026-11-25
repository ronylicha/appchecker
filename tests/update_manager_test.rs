use appchecker::core::{DepInfo, Ecosystem, UpdateSeverity};
use appchecker::deps::{UpdateAction, UpdateManager};
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn dependency(ecosystem: Ecosystem, name: &str, target: &str, dev: bool) -> DepInfo {
    DepInfo {
        name: name.into(),
        ecosystem,
        current_version: "1.0.0".into(),
        latest_version: Some(target.into()),
        update_available: true,
        severity: UpdateSeverity::Major,
        dev_dependency: dev,
        manifest_name: None,
        checked_at: Utc::now(),
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "appchecker-update-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn builds_native_commands_without_a_shell() {
    let cases = [
        (
            Ecosystem::Npm,
            "typescript",
            "5.3.0",
            true,
            "npm",
            vec![
                "install",
                "typescript@5.3.0",
                "--ignore-scripts",
                "--save-dev",
            ],
        ),
        (
            Ecosystem::Cargo,
            "criterion",
            "0.5.0",
            true,
            "cargo",
            vec!["add", "criterion@0.5.0", "--dev"],
        ),
        (
            Ecosystem::Go,
            "example.com/mod",
            "1.2.0",
            false,
            "go",
            vec!["get", "example.com/mod@v1.2.0"],
        ),
        (
            Ecosystem::Composer,
            "phpunit/phpunit",
            "10.5.0",
            true,
            "composer",
            vec![
                "require",
                "phpunit/phpunit:10.5.0",
                "--with-all-dependencies",
                "--no-interaction",
                "--no-scripts",
                "--dev",
            ],
        ),
    ];

    for (ecosystem, name, target, dev, program, args) in cases {
        let plan = UpdateManager::plan(
            Path::new("/tmp/project"),
            &dependency(ecosystem, name, target, dev),
        )
        .unwrap();
        match plan.action {
            UpdateAction::Command {
                program: actual,
                args: actual_args,
            } => {
                assert_eq!(actual, program);
                assert_eq!(actual_args, args);
            }
            other => panic!("expected native command, got {other:?}"),
        }
    }
}

#[test]
fn preserves_cargo_dependency_aliases_during_updates() {
    let mut dep = dependency(Ecosystem::Cargo, "serde", "1.0.0", false);
    dep.manifest_name = Some("my_serde".into());
    let plan = UpdateManager::plan(Path::new("/tmp/project"), &dep).unwrap();

    let UpdateAction::Command { args, .. } = plan.action else {
        panic!("expected cargo command");
    };
    assert_eq!(args, vec!["add", "serde@1.0.0", "--rename", "my_serde"]);
}

#[test]
fn refuses_to_update_without_a_verified_target_version() {
    let mut dep = dependency(Ecosystem::Npm, "express", "5.0.0", false);
    dep.latest_version = None;

    let error = UpdateManager::plan(Path::new("/tmp/project"), &dep).unwrap_err();

    assert!(error.to_string().contains("no target version"));
}

#[test]
fn updates_a_requirements_file_and_verifies_the_result() {
    let root = temp_dir("requirements");
    std::fs::write(
        root.join("requirements.txt"),
        "requests>=2.31.0 # keep this comment, retries [auth]; <safe>\nflask==3.0.0\n",
    )
    .unwrap();
    let dep = dependency(Ecosystem::Pip, "requests", "2.32.0", false);
    let plan = UpdateManager::plan(&root, &dep).unwrap();

    let outcome = UpdateManager::execute(&plan).unwrap();

    let content = std::fs::read_to_string(root.join("requirements.txt")).unwrap();
    assert_eq!(
        content,
        "requests==2.32.0 # keep this comment, retries [auth]; <safe>\nflask==3.0.0\n"
    );
    assert!(outcome.output.contains("requests==2.32.0"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn updates_pyproject_without_reformatting_the_document() {
    let root = temp_dir("pyproject");
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"demo\"\ndependencies = [\n    \"requests>=2.31.0\", # keep formatting\n    \"flask==3.0.0\",\n]\n",
    )
    .unwrap();
    let dep = dependency(Ecosystem::Pip, "requests", "2.32.0", false);
    let plan = UpdateManager::plan(&root, &dep).unwrap();

    UpdateManager::execute(&plan).unwrap();

    let content = std::fs::read_to_string(root.join("pyproject.toml")).unwrap();
    assert!(content.contains("\"requests==2.32.0\", # keep formatting"));
    assert!(content.contains("\"flask==3.0.0\","));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn falls_back_to_requirements_when_pyproject_has_no_declared_dependencies() {
    let root = temp_dir("requirements-fallback");
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\nname = \"demo\"\ndependencies = []\n",
    )
    .unwrap();
    std::fs::write(root.join("requirements.txt"), "requests>=1.0.0\n").unwrap();
    let dep = dependency(Ecosystem::Pip, "requests", "2.0.0", false);
    let plan = UpdateManager::plan(&root, &dep).unwrap();

    UpdateManager::execute(&plan).unwrap();

    assert_eq!(
        std::fs::read_to_string(root.join("requirements.txt")).unwrap(),
        "requests==2.0.0\n"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn verifies_python_updates_by_ecosystem_in_polyglot_projects() {
    let root = temp_dir("polyglot");
    std::fs::write(root.join("requirements.txt"), "requests>=1.0.0\n").unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"requests":"0.0.1"}}"#,
    )
    .unwrap();
    let dep = dependency(Ecosystem::Pip, "requests", "2.0.0", false);
    let plan = UpdateManager::plan(&root, &dep).unwrap();

    UpdateManager::execute(&plan).unwrap();

    assert_eq!(
        std::fs::read_to_string(root.join("requirements.txt")).unwrap(),
        "requests==2.0.0\n"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn rejects_complex_python_specs_without_rewriting_them() {
    let root = temp_dir("complex-python");
    let original =
        "[project]\ndependencies = [\"requests[security]>=1.0.0; python_version >= '3.11'\"]\n";
    std::fs::write(root.join("pyproject.toml"), original).unwrap();
    let dep = dependency(Ecosystem::Pip, "requests", "2.0.0", false);
    let plan = UpdateManager::plan(&root, &dep).unwrap();

    let error = UpdateManager::execute(&plan).unwrap_err();

    assert!(error.to_string().contains("complex Python requirement"));
    assert_eq!(
        std::fs::read_to_string(root.join("pyproject.toml")).unwrap(),
        original
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn updates_gemfile_constraints_before_running_bundle_update() {
    let root = temp_dir("gem-update");
    std::fs::write(
        root.join("Gemfile"),
        "gem \"rubocop\", \">=1.50.0\" # core, web\n",
    )
    .unwrap();
    let dep = dependency(Ecosystem::Gem, "rubocop", "1.60.0", true);
    let mut plan = UpdateManager::plan(&root, &dep).unwrap();
    let script = root.join("bundle");
    std::fs::write(&script, "#!/bin/sh\necho 'bundle update succeeded'\n").unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
    }
    std::fs::set_permissions(&script, permissions).unwrap();
    match &mut plan.action {
        UpdateAction::ManifestThenCommand { program, args, .. } => {
            assert_eq!(args, &["update".to_string(), "rubocop".to_string()]);
            *program = script.to_string_lossy().to_string();
        }
        other => panic!("expected manifest then bundle update, got {other:?}"),
    }

    let outcome = UpdateManager::execute(&plan).unwrap();

    assert!(outcome.output.contains("bundle update succeeded"));
    assert_eq!(
        std::fs::read_to_string(root.join("Gemfile")).unwrap(),
        "gem \"rubocop\", \">=1.60.0\" # core, web\n"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn ignores_non_public_pyproject_entries_when_choosing_the_manifest() {
    let root = temp_dir("non-public-pyproject");
    std::fs::write(
        root.join("pyproject.toml"),
        "[project]\ndependencies = [\"./local.whl\"]\n",
    )
    .unwrap();
    std::fs::write(root.join("requirements.txt"), "requests==1.0.0\n").unwrap();
    let dep = dependency(Ecosystem::Pip, "requests", "2.0.0", false);
    let plan = UpdateManager::plan(&root, &dep).unwrap();

    UpdateManager::execute(&plan).unwrap();

    assert_eq!(
        std::fs::read_to_string(root.join("requirements.txt")).unwrap(),
        "requests==2.0.0\n"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn rejects_compound_gem_constraints_before_rewriting_them() {
    let root = temp_dir("compound-gem");
    let original = "gem \"rails\", \">= 7.0\", \"< 8.0\"\n";
    std::fs::write(root.join("Gemfile"), original).unwrap();
    let dep = dependency(Ecosystem::Gem, "rails", "8.0.0", false);
    let plan = UpdateManager::plan(&root, &dep).unwrap();

    let error = UpdateManager::execute(&plan).unwrap_err();

    assert!(error.to_string().contains("compound Gem constraints"));
    assert_eq!(
        std::fs::read_to_string(root.join("Gemfile")).unwrap(),
        original
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn returns_the_package_manager_error_output() {
    let root = temp_dir("command-error");
    let script = root.join("failing-manager");
    std::fs::write(&script, "#!/bin/sh\necho 'simulated failure' >&2\nexit 7\n").unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
    }
    std::fs::set_permissions(&script, permissions).unwrap();
    let plan = appchecker::deps::UpdatePlan {
        project_path: root.clone(),
        package: "demo".into(),
        ecosystem: Ecosystem::Npm,
        current_version: "1.0.0".into(),
        target_version: "2.0.0".into(),
        action: UpdateAction::Command {
            program: "/bin/sh".into(),
            args: vec![script.to_string_lossy().to_string()],
        },
    };

    let error = UpdateManager::execute(&plan).unwrap_err();

    assert!(error.to_string().contains("simulated failure"));
    assert!(error.to_string().contains("exit status 7"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn cancels_a_hanging_package_manager_without_waiting_for_it() {
    let root = temp_dir("cancel-command");
    let script = root.join("hanging-manager");
    std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
    }
    std::fs::set_permissions(&script, permissions).unwrap();
    let plan = appchecker::deps::UpdatePlan {
        project_path: root.clone(),
        package: "demo".into(),
        ecosystem: Ecosystem::Npm,
        current_version: "1.0.0".into(),
        target_version: "2.0.0".into(),
        action: UpdateAction::Command {
            program: "/bin/sh".into(),
            args: vec![script.to_string_lossy().to_string()],
        },
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_worker = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        cancel_worker.store(true, Ordering::Relaxed);
    });
    let started = std::time::Instant::now();

    let error = UpdateManager::execute_with_cancellation(&plan, Some(cancel)).unwrap_err();

    assert!(error.to_string().contains("cancelled by user"));
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn drains_large_package_manager_output_without_deadlocking() {
    let root = temp_dir("large-output");
    let script = root.join("noisy-manager");
    std::fs::write(
        &script,
        "#!/bin/sh\nawk 'BEGIN { for (i = 0; i < 10000; i++) print \"0123456789\" }'\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
    }
    std::fs::set_permissions(&script, permissions).unwrap();
    let plan = appchecker::deps::UpdatePlan {
        project_path: root.clone(),
        package: "demo".into(),
        ecosystem: Ecosystem::Npm,
        current_version: "1.0.0".into(),
        target_version: "2.0.0".into(),
        action: UpdateAction::Command {
            program: "/bin/sh".into(),
            args: vec![script.to_string_lossy().to_string()],
        },
    };

    let outcome = UpdateManager::execute(&plan).unwrap();

    assert!(outcome.output.len() > 64 * 1024);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn cancellation_does_not_require_python_on_the_path() {
    let root = temp_dir("cancel-without-python");
    let script = root.join("hanging-manager");
    std::fs::write(&script, "#!/bin/sh\n/bin/sleep 3\n").unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
    }
    std::fs::set_permissions(&script, permissions).unwrap();
    let plan = appchecker::deps::UpdatePlan {
        project_path: root.clone(),
        package: "demo".into(),
        ecosystem: Ecosystem::Npm,
        current_version: "1.0.0".into(),
        target_version: "2.0.0".into(),
        action: UpdateAction::Command {
            program: "/bin/sh".into(),
            args: vec![script.to_string_lossy().to_string()],
        },
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_worker = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        cancel_worker.store(true, Ordering::Relaxed);
    });
    let started = std::time::Instant::now();
    let error = UpdateManager::execute_with_cancellation(&plan, Some(cancel)).unwrap_err();
    assert!(error.to_string().contains("cancelled by user"));
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn package_managers_receive_closed_stdin_instead_of_the_tui_terminal() {
    let root = temp_dir("closed-stdin");
    let script = root.join("check-stdin");
    std::fs::write(
        &script,
        "#!/bin/sh\nif [ \"$(readlink /proc/self/fd/0)\" = \"/dev/null\" ]; then\n  echo 'stdin closed'\nelse\n  echo 'stdin inherited' >&2\n  exit 9\nfi\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
    }
    std::fs::set_permissions(&script, permissions).unwrap();
    let plan = appchecker::deps::UpdatePlan {
        project_path: root.clone(),
        package: "demo".into(),
        ecosystem: Ecosystem::Npm,
        current_version: "1.0.0".into(),
        target_version: "2.0.0".into(),
        action: UpdateAction::Command {
            program: "/bin/sh".into(),
            args: vec![script.to_string_lossy().to_string()],
        },
    };
    let outcome = UpdateManager::execute(&plan).unwrap();
    assert_eq!(outcome.output, "stdin closed");
    std::fs::remove_dir_all(root).ok();
}

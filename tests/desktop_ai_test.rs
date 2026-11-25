use appchecker::desktop::ai::{
    AiProvider, ProviderClient, ProviderStatus, ReviewedPatch, apply_reviewed_with_checks,
    parse_analysis_response, prepare_analysis_files,
};
use std::path::PathBuf;
use std::process::Command;

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("appchecker-ai-{name}-{}", uuid::Uuid::new_v4()))
}

fn repository() -> (PathBuf, PathBuf) {
    let root = temp("repo");
    let repository = root.join("repo");
    std::fs::create_dir_all(&repository).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "tests@appchecker.local"],
        vec!["config", "user.name", "AppChecker tests"],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(repository.join("requirements.txt"), "requests==1.0.0\n").unwrap();
    std::fs::write(repository.join("src.py"), "print('before')\n").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-qm", "Initial fixture"]] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );
    }
    (root, repository)
}

#[test]
fn missing_provider_returns_an_actionable_error() {
    let client = ProviderClient::with_program(AiProvider::Claude, "/definitely/missing/claude");
    let error = client.status().unwrap_err().to_string();
    assert!(error.contains("Install Claude Code"));
}

#[test]
fn unauthenticated_provider_is_not_reported_as_ready() {
    let client = ProviderClient::with_program(AiProvider::Codex, "/bin/false");
    assert_eq!(client.status().unwrap(), ProviderStatus::Unauthenticated);
}

#[test]
fn malformed_ai_response_is_rejected() {
    let error = parse_analysis_response("not json").unwrap_err();
    assert!(error.to_string().contains("structured"));
}

#[test]
fn approved_files_are_bounded_copied_and_redacted() {
    let (root, repository) = repository();
    std::fs::write(
        repository.join("settings.py"),
        "API_KEY = 'super-secret-value'\nSAFE = True\n",
    )
    .unwrap();
    let prepared = prepare_analysis_files(
        &repository,
        &[
            PathBuf::from("settings.py"),
            PathBuf::from("requirements.txt"),
        ],
        &root.join("analysis"),
    )
    .unwrap();
    let copied = std::fs::read_to_string(prepared.directory().join("settings.py")).unwrap();
    assert!(!copied.contains("super-secret-value"));
    assert!(copied.contains("SAFE = True"));
    assert!(copied.contains("[REDACTED]"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn patch_cannot_touch_an_unapproved_file() {
    let (root, repository) = repository();
    let patch = "diff --git a/src.py b/src.py\n--- a/src.py\n+++ b/src.py\n@@ -1 +1 @@\n-print('before')\n+print('after')\n";
    let error = ReviewedPatch::review(
        &repository,
        patch,
        &[PathBuf::from("requirements.txt")],
        &root.join("backups"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("outside the approved"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn reviewed_patch_applies_and_rolls_back_only_without_later_edits() {
    let (root, repository) = repository();
    let patch = "diff --git a/src.py b/src.py\n--- a/src.py\n+++ b/src.py\n@@ -1 +1 @@\n-print('before')\n+print('after')\n";
    let reviewed = ReviewedPatch::review(
        &repository,
        patch,
        &[PathBuf::from("src.py")],
        &root.join("backups"),
    )
    .unwrap();
    let applied = reviewed.apply().unwrap();
    assert_eq!(
        std::fs::read_to_string(repository.join("src.py")).unwrap(),
        "print('after')\n"
    );
    applied.rollback().unwrap();
    assert_eq!(
        std::fs::read_to_string(repository.join("src.py")).unwrap(),
        "print('before')\n"
    );

    let reviewed = ReviewedPatch::review(
        &repository,
        patch,
        &[PathBuf::from("src.py")],
        &root.join("backups"),
    )
    .unwrap();
    let applied = reviewed.apply().unwrap();
    std::fs::write(repository.join("src.py"), "print('user edit')\n").unwrap();
    assert!(applied.rollback().is_err());
    assert_eq!(
        std::fs::read_to_string(repository.join("src.py")).unwrap(),
        "print('user edit')\n"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn failed_checks_keep_the_applied_patch_available_for_rollback() {
    let (root, repository) = repository();
    std::fs::write(
        repository.join("package.json"),
        "{\"scripts\":{\"test\":\"exit 1\"}}\n",
    )
    .unwrap();
    for args in [vec!["add", "."], vec!["commit", "-qm", "Add failing check"]] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );
    }
    let patch = "diff --git a/src.py b/src.py\n--- a/src.py\n+++ b/src.py\n@@ -1 +1 @@\n-print('before')\n+print('after')\n";
    let reviewed = ReviewedPatch::review(
        &repository,
        patch,
        &[PathBuf::from("src.py")],
        &root.join("backups"),
    )
    .unwrap();

    let (applied, report) = apply_reviewed_with_checks(reviewed).unwrap();
    assert!(report.contains("failed"));
    applied.rollback().unwrap();
    assert_eq!(
        std::fs::read_to_string(repository.join("src.py")).unwrap(),
        "print('before')\n"
    );
    std::fs::remove_dir_all(root).ok();
}

use appchecker::git::GitScanner;
use std::path::PathBuf;
use std::process::Command;

fn git(repo: &PathBuf, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn detects_staged_same_length_edits_and_nested_untracked_files() {
    let repo = std::env::temp_dir().join(format!("appchecker-git-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(repo.join("nested")).unwrap();
    git(&repo, &["init", "--quiet"]);
    git(&repo, &["config", "user.email", "test@example.invalid"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("version.txt"), "1.0.0\n").unwrap();
    git(&repo, &["add", "version.txt"]);
    git(&repo, &["commit", "--quiet", "-m", "initial"]);

    std::fs::write(repo.join("version.txt"), "2.0.0\n").unwrap();
    std::fs::write(repo.join("nested/new.txt"), "new\n").unwrap();
    std::fs::write(repo.join("nested/untracked.txt"), "untracked\n").unwrap();
    git(&repo, &["add", "version.txt", "nested/new.txt"]);

    let state = GitScanner::scan_repo(&repo).unwrap();

    assert!(state.is_dirty);
    assert!(
        state.staged_count >= 2,
        "staged changes were missed: {state:?}"
    );
    assert!(
        state.untracked_count >= 1,
        "untracked file was missed: {state:?}"
    );
    std::fs::remove_dir_all(repo).ok();
}

#[test]
fn handles_utf8_renames_without_panicking_or_double_counting() {
    let repo =
        std::env::temp_dir().join(format!("appchecker-git-unicode-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet"]);
    git(&repo, &["config", "user.email", "test@example.invalid"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("日本語.txt"), "content\n").unwrap();
    git(&repo, &["add", "日本語.txt"]);
    git(&repo, &["commit", "--quiet", "-m", "initial"]);
    git(&repo, &["mv", "日本語.txt", "new.txt"]);

    let state = GitScanner::scan_repo(&repo).unwrap();

    assert!(state.is_dirty);
    assert_eq!(state.staged_count, 1);
    std::fs::remove_dir_all(repo).ok();
}

#[test]
fn reports_an_error_instead_of_clean_when_git_status_is_unavailable() {
    let repo = std::env::temp_dir().join(format!("appchecker-git-bare-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--bare"]);

    let error = GitScanner::scan_repo(&repo).unwrap_err();

    assert!(error.to_string().contains("working tree"));
    std::fs::remove_dir_all(repo).ok();
}

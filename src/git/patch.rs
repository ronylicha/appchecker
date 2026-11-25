use crate::core::{AppError, Result};
use crate::git::safety::{assert_clean_at, head};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug)]
pub struct ReviewedPatch {
    repository: PathBuf,
    expected_head: String,
    patch_path: PathBuf,
    backup_directory: PathBuf,
    diff_hash: String,
}

#[derive(Debug)]
pub struct AppliedPatch {
    repository: PathBuf,
    expected_head: String,
    original_branch: String,
    remediation_branch: String,
    applied_patch: PathBuf,
    applied_diff: Vec<u8>,
    pub review_hash: String,
}

impl ReviewedPatch {
    pub fn review(
        repository: &Path,
        patch: &str,
        allowed_paths: &[PathBuf],
        backup_root: &Path,
    ) -> Result<Self> {
        if patch.len() > 2 * 1024 * 1024 {
            return Err(AppError::Other(
                "Patch exceeds the 2 MiB safety limit".into(),
            ));
        }
        if patch.trim().is_empty() {
            return Err(AppError::Other("The proposed patch is empty".into()));
        }
        validate_patch_paths(patch, allowed_paths)?;
        let expected_head = head(repository)?;
        assert_clean_at(repository, &expected_head)?;
        let backup_directory = backup_root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&backup_directory)?;
        let patch_path = backup_directory.join("reviewed.patch");
        std::fs::write(&patch_path, patch)?;
        restrict_file(&patch_path)?;
        git(
            repository,
            &["apply", "--check", patch_path.to_str().unwrap()],
        )?;
        Ok(Self {
            repository: repository.to_path_buf(),
            expected_head,
            patch_path,
            backup_directory,
            diff_hash: hash(patch.as_bytes()),
        })
    }

    pub fn repository_path(&self) -> &Path {
        &self.repository
    }

    pub fn diff_hash(&self) -> &str {
        &self.diff_hash
    }

    pub fn apply(self) -> Result<AppliedPatch> {
        assert_clean_at(&self.repository, &self.expected_head)?;
        let reviewed = std::fs::read(&self.patch_path)?;
        if hash(&reviewed) != self.diff_hash {
            return Err(AppError::Other(
                "Reviewed patch changed on disk; review it again".into(),
            ));
        }
        git(
            &self.repository,
            &["apply", "--check", self.patch_path.to_str().unwrap()],
        )?;
        let original_branch =
            String::from_utf8_lossy(&git(&self.repository, &["branch", "--show-current"])?.stdout)
                .trim()
                .to_string();
        if original_branch.is_empty() {
            return Err(AppError::Other(
                "AI remediation requires a named current branch".into(),
            ));
        }
        let remediation_branch = format!(
            "appchecker/remediation-{}",
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        git(&self.repository, &["switch", "-c", &remediation_branch])?;
        if let Err(error) = git(
            &self.repository,
            &[
                "apply",
                "--whitespace=nowarn",
                self.patch_path.to_str().unwrap(),
            ],
        ) {
            git(&self.repository, &["switch", &original_branch]).ok();
            git(&self.repository, &["branch", "-D", &remediation_branch]).ok();
            return Err(error);
        }
        let applied_diff = git(
            &self.repository,
            &["diff", "--binary", "--full-index", "HEAD"],
        )?
        .stdout;
        let applied_patch = self.backup_directory.join("applied.patch");
        std::fs::write(&applied_patch, &applied_diff)?;
        restrict_file(&applied_patch)?;
        Ok(AppliedPatch {
            repository: self.repository,
            expected_head: self.expected_head,
            original_branch,
            remediation_branch,
            applied_patch,
            applied_diff,
            review_hash: self.diff_hash,
        })
    }
}

impl AppliedPatch {
    pub fn rollback(self) -> Result<()> {
        if head(&self.repository)? != self.expected_head {
            return Err(AppError::Other(
                "Rollback refused: repository HEAD changed".into(),
            ));
        }
        let current_branch =
            String::from_utf8_lossy(&git(&self.repository, &["branch", "--show-current"])?.stdout)
                .trim()
                .to_string();
        if current_branch != self.remediation_branch {
            return Err(AppError::Other(
                "Rollback refused: current branch changed".into(),
            ));
        }
        let current_diff = git(
            &self.repository,
            &["diff", "--binary", "--full-index", "HEAD"],
        )?
        .stdout;
        if current_diff != self.applied_diff {
            return Err(AppError::Other(
                "Rollback refused: files changed after the reviewed patch".into(),
            ));
        }
        git(
            &self.repository,
            &[
                "apply",
                "--reverse",
                "--check",
                self.applied_patch.to_str().unwrap(),
            ],
        )?;
        git(
            &self.repository,
            &["apply", "--reverse", self.applied_patch.to_str().unwrap()],
        )?;
        git(&self.repository, &["switch", &self.original_branch])?;
        git(
            &self.repository,
            &["branch", "-D", &self.remediation_branch],
        )?;
        Ok(())
    }
}

fn validate_patch_paths(patch: &str, allowed_paths: &[PathBuf]) -> Result<()> {
    for allowed in allowed_paths {
        validate_relative(allowed)?;
    }
    let mut found = false;
    for line in patch.lines() {
        if line.contains("120000") || line.contains("160000") {
            return Err(AppError::Other(
                "Patches containing symlinks or submodules are rejected".into(),
            ));
        }
        if line.starts_with("rename from ") || line.starts_with("rename to ") {
            return Err(AppError::Other("Patch renames are rejected".into()));
        }
        if let Some(header) = line.strip_prefix("diff --git ") {
            let mut paths = header.split_whitespace();
            let (Some(source), Some(destination), None) =
                (paths.next(), paths.next(), paths.next())
            else {
                return Err(AppError::Other("Malformed diff --git header".into()));
            };
            for header in [source, destination] {
                let Some(path) = header
                    .strip_prefix("a/")
                    .or_else(|| header.strip_prefix("b/"))
                else {
                    return Err(AppError::Other(
                        "Patch paths must be repository-relative".into(),
                    ));
                };
                let path = PathBuf::from(path);
                validate_relative(&path)?;
                if !allowed_paths.contains(&path) {
                    return Err(AppError::Other(format!(
                        "Patch touches '{}' outside the approved file set",
                        path.display()
                    )));
                }
            }
            found = true;
            continue;
        }
        let Some(header) = line
            .strip_prefix("--- ")
            .or_else(|| line.strip_prefix("+++ "))
        else {
            continue;
        };
        if header == "/dev/null" {
            continue;
        }
        let Some(path) = header
            .strip_prefix("a/")
            .or_else(|| header.strip_prefix("b/"))
        else {
            return Err(AppError::Other(format!(
                "Patch path '{header}' must be repository-relative"
            )));
        };
        let path = PathBuf::from(path);
        validate_relative(&path)?;
        if !allowed_paths.contains(&path) {
            return Err(AppError::Other(format!(
                "Patch touches '{}' outside the approved file set",
                path.display()
            )));
        }
        found = true;
    }
    if !found {
        return Err(AppError::Other("No repository file found in patch".into()));
    }
    Ok(())
}

fn validate_relative(path: &Path) -> Result<()> {
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || path
            .components()
            .any(|component| component.as_os_str() == ".git")
    {
        return Err(AppError::Other(format!(
            "Unsafe patch path '{}'",
            path.display()
        )));
    }
    Ok(())
}

fn git(repository: &Path, args: &[&str]) -> Result<Output> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repository)
        .output()?;
    if !output.status.success() {
        return Err(AppError::Git(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output)
}

fn hash(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

#[cfg(unix)]
fn restrict_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) -> Result<()> {
    Ok(())
}

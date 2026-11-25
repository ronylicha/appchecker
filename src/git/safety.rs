use crate::core::{AppError, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct ProjectLock {
    path: PathBuf,
    _file: std::fs::File,
}

impl ProjectLock {
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            restrict_directory(parent)?;
        }
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    AppError::Other("Another update is already running for this project".into())
                } else {
                    AppError::Io(error)
                }
            })?;
        Ok(Self {
            path: path.to_path_buf(),
            _file: file,
        })
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
    }
}

#[derive(Debug)]
struct BackupFile {
    relative: PathBuf,
    original: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct UpdateBackup {
    directory: PathBuf,
    files: Vec<BackupFile>,
}

impl UpdateBackup {
    pub fn create(repository: &Path, root: &Path) -> Result<Self> {
        let directory = root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&directory)?;
        restrict_directory(&directory)?;
        let mut backup = Self {
            directory,
            files: Vec::new(),
        };
        for relative in manifest_files(repository)? {
            let source = repository.join(&relative);
            let original = std::fs::read(&source)?;
            let destination = backup.directory.join(&relative);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&destination, &original)?;
            restrict_file(&destination)?;
            backup.files.push(BackupFile {
                relative,
                original: Some(original),
                after: None,
            });
        }
        Ok(backup)
    }

    pub fn capture_after(&mut self, repository: &Path) -> Result<()> {
        let current = manifest_files(repository)?;
        for relative in current {
            if !self.files.iter().any(|file| file.relative == relative) {
                self.files.push(BackupFile {
                    relative,
                    original: None,
                    after: None,
                });
            }
        }
        for file in &mut self.files {
            file.after = match std::fs::read(repository.join(&file.relative)) {
                Ok(content) => Some(content),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
        }
        Ok(())
    }

    pub fn rollback_if_unchanged(self, repository: &Path) -> Result<PathBuf> {
        for file in &self.files {
            let current = match std::fs::read(repository.join(&file.relative)) {
                Ok(content) => Some(content),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            if current != file.after {
                return Err(AppError::Other(format!(
                    "Rollback refused: '{}' changed after the update",
                    file.relative.display()
                )));
            }
        }
        for file in &self.files {
            let path = repository.join(&file.relative);
            match &file.original {
                Some(content) => {
                    std::fs::write(&path, content)?;
                }
                None => match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                },
            }
        }
        Ok(self.directory)
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

pub fn head(repository: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repository)
        .output()?;
    if !output.status.success() {
        return Err(AppError::Git(format!(
            "Cannot read HEAD in '{}': {}",
            repository.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn assert_clean_at(repository: &Path, expected_head: &str) -> Result<()> {
    let actual = head(repository)?;
    if actual != expected_head {
        return Err(AppError::Git(format!(
            "Repository HEAD changed from {expected_head} to {actual}; review the updates again"
        )));
    }
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .current_dir(repository)
        .output()?;
    if !output.status.success() {
        return Err(AppError::Git(format!(
            "Cannot inspect '{}': {}",
            repository.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    if !output.stdout.is_empty() {
        return Err(AppError::Git(
            "Repository has uncommitted changes; commit or stash them before updating".into(),
        ));
    }
    Ok(())
}

fn manifest_files(repository: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(repository)
        .min_depth(1)
        .max_depth(5)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some(".git" | "node_modules" | "target" | "vendor" | ".venv")
            )
        })
    {
        let entry = entry.map_err(|error| AppError::Other(error.to_string()))?;
        if !entry.file_type().is_file() || !is_manifest(entry.path()) {
            continue;
        }
        files.push(entry.path().strip_prefix(repository).unwrap().to_path_buf());
    }
    files.sort();
    Ok(files)
}

fn is_manifest(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(
            "package.json"
                | "package-lock.json"
                | "npm-shrinkwrap.json"
                | "yarn.lock"
                | "pnpm-lock.yaml"
                | "Cargo.toml"
                | "Cargo.lock"
                | "requirements.txt"
                | "pyproject.toml"
                | "poetry.lock"
                | "uv.lock"
                | "Pipfile"
                | "Pipfile.lock"
                | "go.mod"
                | "go.sum"
                | "composer.json"
                | "composer.lock"
                | "Gemfile"
                | "Gemfile.lock"
        )
    ) || path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".gemspec"))
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> Result<()> {
    Ok(())
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

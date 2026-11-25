use crate::config::ConfigLoader;
use crate::core::{AppConfig, AppError, Project, ProjectId, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDraft {
    pub path: PathBuf,
    pub name: String,
    pub tags: Vec<String>,
    pub pinned: bool,
}

pub struct ProjectManager;

impl ProjectManager {
    pub fn add(config: &mut AppConfig, draft: ProjectDraft) -> Result<Project> {
        let draft = Self::normalize_draft(draft)?;
        Self::ensure_unique_path(config, &draft.path, None)?;

        let mut project = Project::new(draft.name, draft.path);
        project.tags = draft.tags;
        project.pinned = draft.pinned;
        config.projects.push(project.clone());
        Self::sort(config);
        Ok(project)
    }

    pub fn update(
        config: &mut AppConfig,
        project_id: ProjectId,
        draft: ProjectDraft,
    ) -> Result<Project> {
        let draft = Self::normalize_draft(draft)?;
        Self::ensure_unique_path(config, &draft.path, Some(project_id))?;
        let index = Self::index_of(config, project_id)?;
        let current = &config.projects[index];
        let updated = Project {
            id: current.id,
            name: draft.name,
            path: draft.path,
            tags: draft.tags,
            pinned: draft.pinned,
            notification_overrides: current.notification_overrides.clone(),
            added_at: current.added_at,
            last_scanned_at: current.last_scanned_at,
        };
        config.projects[index] = updated.clone();
        Self::sort(config);
        Ok(updated)
    }

    pub fn find(config: &AppConfig, selector: &str) -> Result<Project> {
        let path_like = selector == "~"
            || selector == "."
            || selector == ".."
            || selector.starts_with("~/")
            || selector.starts_with('/')
            || selector.starts_with("./")
            || selector.starts_with("../")
            || selector.contains('/');
        if path_like {
            let expanded = if selector == "~" {
                dirs::home_dir().unwrap_or_else(|| PathBuf::from(selector))
            } else if selector.starts_with("~/") {
                dirs::home_dir()
                    .map(|home| home.join(&selector[2..]))
                    .unwrap_or_else(|| PathBuf::from(selector))
            } else {
                PathBuf::from(selector)
            };
            let path = std::fs::canonicalize(&expanded).unwrap_or(expanded);
            if let Some(project) = config
                .projects
                .iter()
                .find(|project| Self::normalize_existing_path(&project.path) == path)
            {
                return Ok(project.clone());
            }
        }
        let matches: Vec<&Project> = config
            .projects
            .iter()
            .filter(|project| project.name == selector)
            .collect();
        match matches.as_slice() {
            [project] => Ok((*project).clone()),
            [] => Err(AppError::Config(format!(
                "No project found matching '{selector}'"
            ))),
            _ => Err(AppError::Config(format!(
                "Project name '{selector}' is ambiguous; use its full path"
            ))),
        }
    }

    pub fn remove(config: &mut AppConfig, project_id: ProjectId) -> Result<Project> {
        let index = Self::index_of(config, project_id)?;
        Ok(config.projects.remove(index))
    }

    pub fn toggle_pin(config: &mut AppConfig, project_id: ProjectId) -> Result<bool> {
        let index = Self::index_of(config, project_id)?;
        let pinned = !config.projects[index].pinned;
        config.projects[index].pinned = pinned;
        Self::sort(config);
        Ok(pinned)
    }

    pub fn discover(config: &mut AppConfig, paths: &[PathBuf], depth: usize) -> Vec<Project> {
        let existing: Vec<PathBuf> = config.projects.iter().map(|p| p.path.clone()).collect();
        ConfigLoader::auto_discover(paths, depth, &existing)
            .into_iter()
            .filter_map(|path| {
                Self::add(
                    config,
                    ProjectDraft {
                        path,
                        name: String::new(),
                        tags: Vec::new(),
                        pinned: false,
                    },
                )
                .ok()
            })
            .collect()
    }

    fn normalize_draft(mut draft: ProjectDraft) -> Result<ProjectDraft> {
        draft.path = Self::normalize_path(&draft.path)?;
        if draft.name.trim().is_empty() {
            draft.name = draft
                .path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "unnamed".into());
        }
        draft.name = draft.name.trim().to_string();
        if draft.name.is_empty() {
            return Err(AppError::Config("Project name cannot be empty".into()));
        }
        draft.tags = draft
            .tags
            .into_iter()
            .map(|tag| tag.trim().to_string())
            .filter(|tag| !tag.is_empty())
            .fold(Vec::new(), |mut tags, tag| {
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
                tags
            });
        Ok(draft)
    }

    fn normalize_path(path: &Path) -> Result<PathBuf> {
        let expanded = if path == Path::new("~") {
            dirs::home_dir()
                .ok_or_else(|| AppError::Config("Cannot resolve the home directory".into()))?
        } else if path.starts_with("~/") {
            dirs::home_dir()
                .ok_or_else(|| AppError::Config("Cannot resolve the home directory".into()))?
                .join(path.strip_prefix("~/").unwrap())
        } else {
            path.to_path_buf()
        };
        let canonical = std::fs::canonicalize(&expanded).map_err(|error| {
            AppError::Config(format!(
                "Cannot access project directory '{}': {error}",
                expanded.display()
            ))
        })?;
        if !canonical.is_dir() {
            return Err(AppError::Config(format!(
                "'{}' is not a directory",
                canonical.display()
            )));
        }
        if !canonical.join(".git").exists() {
            return Err(AppError::Config(format!(
                "'{}' is not a Git repository (missing .git)",
                canonical.display()
            )));
        }
        Ok(canonical)
    }

    fn ensure_unique_path(
        config: &AppConfig,
        path: &Path,
        except: Option<ProjectId>,
    ) -> Result<()> {
        if config.projects.iter().any(|project| {
            Self::normalize_existing_path(&project.path) == path && Some(project.id) != except
        }) {
            return Err(AppError::Config(format!(
                "Project '{}' is already monitored",
                path.display()
            )));
        }
        Ok(())
    }

    fn index_of(config: &AppConfig, project_id: ProjectId) -> Result<usize> {
        config
            .projects
            .iter()
            .position(|project| project.id == project_id)
            .ok_or_else(|| AppError::Config("Project not found".into()))
    }

    fn normalize_existing_path(path: &Path) -> PathBuf {
        let expanded = if path == Path::new("~") {
            dirs::home_dir().unwrap_or_else(|| path.to_path_buf())
        } else if path.starts_with("~/") {
            dirs::home_dir()
                .map(|home| home.join(path.strip_prefix("~/").unwrap()))
                .unwrap_or_else(|| path.to_path_buf())
        } else {
            path.to_path_buf()
        };
        std::fs::canonicalize(&expanded).unwrap_or(expanded)
    }

    fn sort(config: &mut AppConfig) {
        config.projects.sort_by(|left, right| {
            right
                .pinned
                .cmp(&left.pinned)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
    }
}

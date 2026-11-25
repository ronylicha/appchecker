use crate::core::{AppConfig, AppError, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Loads and saves application configuration from TOML files.
pub struct ConfigLoader;

impl ConfigLoader {
    /// Get the default config file path: ~/.config/appchecker/config.toml
    pub fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("appchecker")
            .join("config.toml")
    }

    /// Get the default database path: ~/.local/share/appchecker/appchecker.db
    pub fn default_db_path() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("appchecker")
            .join("appchecker.db")
    }

    /// Load config from the given path, or return defaults if file doesn't exist.
    pub fn load(path: &Path) -> Result<AppConfig> {
        if !path.exists() {
            debug!("Config file not found at {:?}, using defaults", path);
            return Ok(AppConfig::default());
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| AppError::Config(format!("Failed to read config: {}", e)))?;

        let config: AppConfig = toml::from_str(&content)
            .map_err(|e| AppError::Config(format!("Failed to parse config: {}", e)))?;

        info!("Loaded config from {:?}", path);
        Ok(config)
    }

    /// Save config to the given path, creating parent directories if needed.
    pub fn save(config: &AppConfig, path: &Path) -> Result<()> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Config(format!("Failed to create config dir: {}", e)))?;
        }

        let content = toml::to_string_pretty(config)
            .map_err(|e| AppError::Config(format!("Failed to serialize config: {}", e)))?;
        let temporary = path.with_extension(format!("toml-{}.tmp", uuid::Uuid::new_v4()));
        std::fs::write(&temporary, content)
            .map_err(|e| AppError::Config(format!("Failed to write config: {}", e)))?;
        if path.exists() {
            let permissions = std::fs::metadata(path)
                .map_err(|e| AppError::Config(format!("Failed to read config permissions: {}", e)))?
                .permissions();
            std::fs::set_permissions(&temporary, permissions).map_err(|e| {
                AppError::Config(format!("Failed to preserve config permissions: {}", e))
            })?;
        }
        std::fs::rename(&temporary, path)
            .map_err(|e| AppError::Config(format!("Failed to replace config: {}", e)))?;

        info!("Saved config to {:?}", path);
        Ok(())
    }

    /// Auto-discover projects by scanning directories for .git folders.
    pub fn auto_discover(
        search_paths: &[PathBuf],
        max_depth: usize,
        existing_paths: &[PathBuf],
    ) -> Vec<PathBuf> {
        Self::auto_discover_with_cancel(
            search_paths,
            max_depth,
            existing_paths,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    }

    pub fn auto_discover_with_cancel(
        search_paths: &[PathBuf],
        max_depth: usize,
        existing_paths: &[PathBuf],
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Vec<PathBuf> {
        let mut discovered = Vec::new();
        let existing: HashSet<PathBuf> = existing_paths
            .iter()
            .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
            .collect();
        let mut seen = HashSet::new();

        for search_path in search_paths {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let expanded = if search_path == Path::new("~") {
                dirs::home_dir().unwrap_or_else(|| search_path.clone())
            } else if search_path.starts_with("~/") {
                dirs::home_dir()
                    .map(|home| home.join(search_path.strip_prefix("~/").unwrap()))
                    .unwrap_or_else(|| search_path.clone())
            } else {
                search_path.clone()
            };
            let search_path = std::fs::canonicalize(&expanded).unwrap_or(expanded);
            if !search_path.exists() {
                debug!("Auto-discover path does not exist: {:?}", search_path);
                continue;
            }

            let walker = walkdir::WalkDir::new(&search_path)
                .max_depth(max_depth)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    // Skip hidden directories except .git detection
                    let name = e.file_name().to_string_lossy();
                    if e.depth() > 0 && name.starts_with('.') && name != ".git" {
                        return false;
                    }
                    // Skip common non-project directories
                    !matches!(
                        name.as_ref(),
                        "node_modules" | "target" | "vendor" | "__pycache__" | ".venv" | "venv"
                    )
                });

            for entry in walker.filter_map(|e| e.ok()) {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                if (entry.file_type().is_dir() || entry.file_type().is_file())
                    && entry.file_name() == ".git"
                {
                    let project_dir = entry.path().parent().unwrap().to_path_buf();
                    let project_dir = std::fs::canonicalize(&project_dir).unwrap_or(project_dir);
                    if !existing.contains(&project_dir) && seen.insert(project_dir.clone()) {
                        discovered.push(project_dir);
                    }
                }
            }
        }

        info!("Auto-discovered {} new projects", discovered.len());
        discovered
    }
}

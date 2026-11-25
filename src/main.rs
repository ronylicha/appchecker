mod cli;
mod config;
mod core;
mod deps;
mod git;
mod notify;
mod persist;
mod tui;
mod watcher;

use clap::Parser;
use cli::commands::{Cli, Commands};
use config::ConfigLoader;
use core::{AppConfig, Project, ProjectHealth};
use persist::Store;
use std::path::PathBuf;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::new("appchecker=debug")
    } else {
        EnvFilter::new("appchecker=info")
    };
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Resolve config path
    let config_path = if cli.config.to_string_lossy().starts_with('~') {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("appchecker/config.toml")
    } else {
        cli.config.clone()
    };

    match cli.command {
        Some(Commands::Add {
            path,
            name,
            tags,
            pin,
        }) => {
            let mut config = ConfigLoader::load(&config_path)?;
            let project_name = name.unwrap_or_else(|| {
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unnamed".to_string())
            });

            let mut project = Project::new(project_name, path.clone());
            project.pinned = pin;
            if let Some(tag_str) = tags {
                project.tags = tag_str.split(',').map(|t| t.trim().to_string()).collect();
            }

            let display_name = project.name.clone();
            config.projects.push(project);
            ConfigLoader::save(&config, &config_path)?;
            println!("Added project: {} ({})", display_name, path.display());
        }

        Some(Commands::Remove { project }) => {
            let mut config = ConfigLoader::load(&config_path)?;
            let before = config.projects.len();
            config
                .projects
                .retain(|p| p.name != project && p.path.to_string_lossy() != project);
            if config.projects.len() < before {
                ConfigLoader::save(&config, &config_path)?;
                println!("Removed project matching '{}'", project);
            } else {
                println!("No project found matching '{}'", project);
            }
        }

        Some(Commands::List { detailed, tag }) => {
            let config = ConfigLoader::load(&config_path)?;
            let projects: Vec<&Project> = config
                .projects
                .iter()
                .filter(|p| tag.as_ref().map(|t| p.tags.contains(t)).unwrap_or(true))
                .collect();

            if projects.is_empty() {
                println!("No projects monitored. Use 'appchecker add <path>' to add one.");
            } else {
                for p in &projects {
                    let pin = if p.pinned { "📌 " } else { "   " };
                    if detailed {
                        println!(
                            "{}{}  path={}  tags=[{}]  added={}",
                            pin,
                            p.name,
                            p.path.display(),
                            p.tags.join(", "),
                            p.added_at.format("%Y-%m-%d"),
                        );
                    } else {
                        println!("{}{}", pin, p.name);
                    }
                }
            }
        }

        Some(Commands::Check { project, format }) => {
            let config = ConfigLoader::load(&config_path)?;
            let targets: Vec<&Project> = config
                .projects
                .iter()
                .filter(|p| {
                    project
                        .as_ref()
                        .map(|name| p.name == *name || p.path.to_string_lossy() == *name)
                        .unwrap_or(true)
                })
                .collect();

            let mut results = Vec::new();
            for proj in &targets {
                let git_state = git::GitScanner::scan_repo_async(proj.path.clone())
                    .await
                    .ok();
                let deps = deps::DepsParser::detect_and_parse(&proj.path);
                let outdated = deps.iter().filter(|d| d.update_available).count();

                results.push((proj.name.clone(), git_state, deps.len(), outdated));
            }

            match format.as_str() {
                "json" => {
                    let json: Vec<serde_json::Value> = results
                        .iter()
                        .map(|(name, gs, total_deps, outdated)| {
                            serde_json::json!({
                                "project": name,
                                "branch": gs.as_ref().and_then(|g| g.current_branch.clone()),
                                "dirty": gs.as_ref().map(|g| g.is_dirty).unwrap_or(false),
                                "total_deps": total_deps,
                                "outdated_deps": outdated,
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&json)?);
                }
                _ => {
                    for (name, gs, total_deps, outdated) in &results {
                        let branch = gs
                            .as_ref()
                            .and_then(|g| g.current_branch.clone())
                            .unwrap_or_else(|| "?".into());
                        let dirty = if gs.as_ref().map(|g| g.is_dirty).unwrap_or(false) {
                            " [dirty]"
                        } else {
                            ""
                        };
                        println!(
                            "{:<30} branch={}{} deps={} outdated={}",
                            name, branch, dirty, total_deps, outdated
                        );
                    }
                }
            }
        }

        Some(Commands::Discover {
            paths,
            depth,
            dry_run,
        }) => {
            let config = ConfigLoader::load(&config_path)?;
            let search_paths: Vec<PathBuf> = paths
                .map(|p| p.split(',').map(|s| PathBuf::from(s.trim())).collect())
                .unwrap_or_else(|| config.auto_discover_paths.clone());

            let existing: Vec<PathBuf> = config.projects.iter().map(|p| p.path.clone()).collect();
            let discovered = ConfigLoader::auto_discover(&search_paths, depth, &existing);

            if discovered.is_empty() {
                println!("No new projects discovered.");
            } else if dry_run {
                println!("Would add {} projects:", discovered.len());
                for p in &discovered {
                    println!("  {}", p.display());
                }
            } else {
                let mut config = config;
                for path in &discovered {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "unnamed".into());
                    config.projects.push(Project::new(name, path.clone()));
                }
                ConfigLoader::save(&config, &config_path)?;
                println!("Added {} discovered projects.", discovered.len());
            }
        }

        Some(Commands::Daemon {
            foreground,
            interval,
        }) => {
            info!(
                "Starting daemon mode (foreground={}, interval={}s)",
                foreground, interval
            );
            let config = ConfigLoader::load(&config_path)?;
            let db_path = ConfigLoader::default_db_path();
            let store = Store::open(&db_path)?;

            // Save all projects to DB
            for proj in &config.projects {
                store.upsert_project(proj)?;
            }

            let notification_router =
                std::sync::Arc::new(notify::NotificationRouter::new(config.clone()));

            // Start filesystem watcher for reactive scanning
            let project_paths: Vec<(core::ProjectId, PathBuf)> = config
                .projects
                .iter()
                .map(|p| (p.id, p.path.clone()))
                .collect();

            let (fs_watcher, mut watch_rx) = watcher::FsWatcher::new(500)?;
            fs_watcher.start_watching(&project_paths, 500)?;
            info!("Filesystem watcher started for reactive scanning");

            // Build static path lookup for watcher task (Send-safe)
            let path_lookup: std::collections::HashMap<core::ProjectId, PathBuf> = project_paths
                .iter()
                .map(|(id, p)| (*id, p.clone()))
                .collect();

            // Store is Send+Sync via internal mpsc channel — clone the handle
            let store_clone = store.clone();

            // Spawn reactive watcher task
            tokio::spawn(async move {
                loop {
                    match watch_rx.recv().await {
                        Ok(event) => match event {
                            watcher::WatchEvent::GitChanged {
                                project_id,
                                path: _,
                            } => {
                                debug!("Reactive git scan triggered for {:?}", project_id);
                                if let Some(proj_path) = path_lookup.get(&project_id) {
                                    match git::GitScanner::scan_repo_async(proj_path.clone()).await
                                    {
                                        Ok(state) => {
                                            let _ = store_clone.save_git_state(project_id, &state);
                                        }
                                        Err(e) => warn!("Reactive git scan failed: {}", e),
                                    }
                                }
                            }
                            watcher::WatchEvent::ManifestChanged {
                                project_id,
                                path: _,
                            } => {
                                debug!("Reactive deps scan triggered for {:?}", project_id);
                                if let Some(proj_path) = path_lookup.get(&project_id) {
                                    let deps = deps::DepsParser::detect_and_parse(proj_path);
                                    if !deps.is_empty() {
                                        let _ = store_clone.save_deps(project_id, &deps);
                                    }
                                }
                            }
                            watcher::WatchEvent::ProjectLost { project_id } => {
                                warn!("Project directory lost: {:?}", project_id);
                            }
                        },
                        Err(_) => break, // Channel closed
                    }
                }
            });

            // Periodic full scan loop (fallback + registry checks)
            loop {
                info!(
                    "Running periodic scan cycle for {} projects",
                    config.projects.len()
                );
                for proj in &config.projects {
                    // Git scan
                    match git::GitScanner::scan_repo_async(proj.path.clone()).await {
                        Ok(state) => {
                            store.save_git_state(proj.id, &state)?;
                        }
                        Err(e) => {
                            tracing::warn!("Git scan failed for {}: {}", proj.name, e);
                        }
                    }

                    // Deps scan + registry check
                    let local_deps = deps::DepsParser::detect_and_parse(&proj.path);
                    if !local_deps.is_empty() {
                        let registry = deps::RegistryClient::new(
                            config.registry_urls.clone(),
                            config.air_gap_mode,
                        );
                        let checked_deps = registry
                            .check_batch(&local_deps, config.max_concurrent_scans)
                            .await;

                        // Queue notifications for outdated deps
                        for dep in &checked_deps {
                            if dep.update_available {
                                notification_router.queue(
                                    proj.id,
                                    dep.severity,
                                    format!("{} update available", dep.name),
                                    format!(
                                        "{} → {}",
                                        dep.current_version,
                                        dep.latest_version.as_deref().unwrap_or("?")
                                    ),
                                );
                            }
                        }

                        store.save_deps(proj.id, &checked_deps)?;
                    }
                }

                // Flush pending notifications
                notification_router.flush();

                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
            }
        }

        Some(Commands::Config {
            reset,
            path: show_path,
        }) => {
            if show_path {
                println!("{}", config_path.display());
            } else if reset {
                let default = AppConfig::default();
                ConfigLoader::save(&default, &config_path)?;
                println!("Config reset to defaults at {}", config_path.display());
            } else {
                let config = ConfigLoader::load(&config_path)?;
                println!("{}", toml::to_string_pretty(&config)?);
            }
        }

        Some(Commands::Tui) | None => {
            let config = ConfigLoader::load(&config_path)?;
            let db_path = ConfigLoader::default_db_path();

            // Build initial health data from DB or fresh scan
            let health_data: Vec<ProjectHealth> = if db_path.exists() {
                let store = Store::open(&db_path)?;
                config
                    .projects
                    .iter()
                    .map(|p| {
                        let git_state = store.get_git_state(p.id).ok().flatten();
                        let (outdated, security, major) =
                            store.get_project_health_summary(p.id).unwrap_or((0, 0, 0));
                        ProjectHealth {
                            project_id: p.id,
                            git_state,
                            outdated_deps_count: outdated,
                            security_updates_count: security,
                            major_updates_count: major,
                            last_scan_age_secs: None,
                            scan_error: None,
                        }
                    })
                    .collect()
            } else {
                config
                    .projects
                    .iter()
                    .map(|p| ProjectHealth {
                        project_id: p.id,
                        git_state: None,
                        outdated_deps_count: 0,
                        security_updates_count: 0,
                        major_updates_count: 0,
                        last_scan_age_secs: None,
                        scan_error: None,
                    })
                    .collect()
            };

            let mut app = tui::TuiApp::new(config, health_data);
            app.run()?;
        }
    }

    Ok(())
}

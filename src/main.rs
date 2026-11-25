mod cli;

use appchecker::{config, core, deps, git, notify, persist, tui, watcher};
use clap::Parser;
use cli::commands::{Cli, Commands};
use config::{ConfigLoader, ProjectDraft, ProjectManager};
use core::{AppConfig, CheckResult, Project};
use persist::Store;
use std::path::PathBuf;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(async_main());
    runtime.shutdown_background();
    result
}

async fn async_main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let tui_mode = matches!(&cli.command, Some(Commands::Tui) | None);
    let filter = if tui_mode {
        EnvFilter::new("off")
    } else if cli.verbose {
        EnvFilter::new("appchecker=debug")
    } else {
        EnvFilter::new("appchecker=info")
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();

    // Resolve config path
    let raw_config_path = cli.config.to_string_lossy();
    let config_path = if raw_config_path == "~/.config/appchecker/config.toml" {
        ConfigLoader::default_config_path()
    } else if raw_config_path == "~" || raw_config_path.starts_with("~/") {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        if raw_config_path == "~" {
            home
        } else {
            home.join(&raw_config_path[2..])
        }
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
            let project = ProjectManager::add(
                &mut config,
                ProjectDraft {
                    path: path.clone(),
                    name: name.unwrap_or_default(),
                    tags: tags
                        .map(|tags| tags.split(',').map(|tag| tag.trim().to_string()).collect())
                        .unwrap_or_default(),
                    pinned: pin,
                },
            )?;
            ConfigLoader::save(&config, &config_path)?;
            println!(
                "Added project: {} ({})",
                project.name,
                project.path.display()
            );
        }

        Some(Commands::Remove { project }) => {
            let mut config = ConfigLoader::load(&config_path)?;
            let selected = ProjectManager::find(&config, &project)?;
            let removed = ProjectManager::remove(&mut config, selected.id)?;
            ConfigLoader::save(&config, &config_path)?;
            let store = Store::open(&ConfigLoader::default_db_path())?;
            store.remove_project(removed.id)?;
            store.shutdown();
            println!("Removed project: {}", removed.name);
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
            let selected_project;
            let targets: Vec<&Project> = match &project {
                Some(selector) => {
                    selected_project = ProjectManager::find(&config, selector)?;
                    vec![&selected_project]
                }
                None => config.projects.iter().collect(),
            };

            let registry =
                deps::RegistryClient::new(config.registry_urls.clone(), config.air_gap_mode);
            let mut results = Vec::new();
            for proj in &targets {
                let git_result = git::GitScanner::scan_repo_async(proj.path.clone()).await;
                let git_state = git_result.as_ref().ok();
                let git_error = git_result.as_ref().err().map(|error| error.to_string());
                let report = deps::DepsParser::detect_and_report(&proj.path);
                let mut checked = registry
                    .check_batch_detailed(&report.dependencies, config.max_concurrent_scans)
                    .await;
                checked.manifest_errors = report.errors;
                checked.non_public_skipped = report.non_public_skipped;
                let outdated = checked
                    .dependencies
                    .iter()
                    .filter(|dependency| dependency.update_available)
                    .count();
                results.push(CheckResult {
                    project: proj.name.clone(),
                    branch: git_state.and_then(|state| state.current_branch.clone()),
                    dirty: git_state.map(|state| state.is_dirty).unwrap_or(false),
                    total_deps: checked.dependencies.len(),
                    outdated_deps: outdated,
                    registry_errors: checked.errors.len(),
                    non_public_skipped: checked.non_public_skipped,
                    air_gap_mode: config.air_gap_mode,
                    git_error,
                    registry_error_details: checked
                        .errors
                        .into_iter()
                        .map(|error| {
                            format!("{} ({}): {}", error.package, error.ecosystem, error.message)
                        })
                        .collect(),
                    manifest_errors: checked
                        .manifest_errors
                        .iter()
                        .map(|error| error.to_string())
                        .collect(),
                });
            }

            match format.as_str() {
                "json" => {
                    println!("{}", serde_json::to_string_pretty(&results)?);
                }
                _ => {
                    for result in &results {
                        let branch = result.branch.as_deref().unwrap_or("?");
                        let dirty = if result.dirty { " [dirty]" } else { "" };
                        println!(
                            "{:<30} branch={}{} deps={} outdated={} registry_errors={} private_skipped={} air_gap={}",
                            result.project,
                            branch,
                            dirty,
                            result.total_deps,
                            result.outdated_deps,
                            result.registry_errors,
                            result.non_public_skipped,
                            result.air_gap_mode
                        );
                        if let Some(error) = &result.git_error {
                            println!("  git_error: {error}");
                        }
                        for error in &result.registry_error_details {
                            println!("  registry_error: {error}");
                        }
                        for error in &result.manifest_errors {
                            println!("  manifest_error: {error}");
                        }
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
            let depth = depth.unwrap_or(config.max_auto_discover_depth);
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
                let added = ProjectManager::discover(&mut config, &search_paths, depth);
                ConfigLoader::save(&config, &config_path)?;
                println!("Added {} discovered projects.", added.len());
            }
        }

        Some(Commands::Daemon {
            foreground,
            interval,
        }) => {
            let config = ConfigLoader::load(&config_path)?;
            let interval = interval.unwrap_or(config.scan_interval_secs).max(1);
            if !foreground {
                let mut command = std::process::Command::new(std::env::current_exe()?);
                command
                    .arg("--config")
                    .arg(&config_path)
                    .arg("daemon")
                    .arg("--foreground")
                    .arg("--interval")
                    .arg(interval.to_string())
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    command.process_group(0);
                }
                command.spawn()?;
                println!("AppChecker daemon started in the background");
                return Ok(());
            }
            info!(
                "Starting daemon mode (foreground={}, interval={}s)",
                foreground, interval
            );
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
            let reactive_registry =
                deps::RegistryClient::new(config.registry_urls.clone(), config.air_gap_mode);
            let reactive_max_concurrent = config.max_concurrent_scans;

            // Spawn reactive watcher task
            tokio::spawn(async move {
                loop {
                    let event = match watch_rx.recv().await {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                            tracing::warn!(
                                "Filesystem watcher lagged by {count} events; continuing"
                            );
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    match event {
                        watcher::WatchEvent::GitChanged {
                            project_id,
                            path: _,
                        } => {
                            debug!("Reactive git scan triggered for {:?}", project_id);
                            if let Some(proj_path) = path_lookup.get(&project_id) {
                                match git::GitScanner::scan_repo_async(proj_path.clone()).await {
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
                                let report = deps::DepsParser::detect_and_report(proj_path);
                                for error in &report.errors {
                                    warn!("Manifest error: {error}");
                                }
                                let checked = reactive_registry
                                    .check_batch(&report.dependencies, reactive_max_concurrent)
                                    .await;
                                let _ = store_clone.save_deps(project_id, &checked);
                            }
                        }
                        watcher::WatchEvent::ProjectLost { project_id } => {
                            warn!("Project directory lost: {:?}", project_id);
                        }
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
                    let report = deps::DepsParser::detect_and_report(&proj.path);
                    for error in &report.errors {
                        tracing::warn!("Manifest error: {error}");
                    }
                    let registry = deps::RegistryClient::new(
                        config.registry_urls.clone(),
                        config.air_gap_mode,
                    );
                    let checked_deps = registry
                        .check_batch(&report.dependencies, config.max_concurrent_scans)
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
            let store = Store::open(&db_path)?;
            let mut app = tui::TuiApp::new(config, config_path, store);
            app.run()?;
        }
    }

    Ok(())
}

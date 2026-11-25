use super::DesktopApp;
use super::app::{DesktopModal, UpdateScope};
use crate::core::{AppError, Project, ProjectId, Result};
use crate::deps::{
    BatchCheck, DepsParser, RegistryClient, UpdateManager, UpdatePlan, Vulnerability,
};
use crate::git::GitScanner;
use crate::git::safety::{ProjectLock, UpdateBackup, assert_clean_at, head};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone)]
pub(crate) struct PendingUpdate {
    pub project_id: ProjectId,
    pub plan: UpdatePlan,
    pub expected_head: String,
}

pub(crate) enum BackgroundEvent {
    Scan {
        project_id: ProjectId,
        check: BatchCheck,
        git_error: Option<String>,
    },
    Vulnerabilities {
        project_id: ProjectId,
        reports: Vec<(String, std::result::Result<Vec<Vulnerability>, String>)>,
    },
    Update {
        project_id: ProjectId,
        package: String,
        result: std::result::Result<String, String>,
    },
    ProjectFinished {
        project_id: ProjectId,
        message: String,
    },
    UpdatesFinished {
        summary: String,
        projects: Vec<ProjectId>,
    },
}

impl DesktopApp {
    pub fn refresh_project(&mut self, project_id: ProjectId) {
        if self.scanning.contains(&project_id) {
            return;
        }
        let Some(project) = self.project(project_id) else {
            return;
        };
        self.scanning.insert(project_id);
        self.auditing.insert(project_id);
        let registry =
            RegistryClient::new(self.config.registry_urls.clone(), self.config.air_gap_mode);
        let max_concurrent = self.config.max_concurrent_scans;
        let sender = self.background_sender.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("scan runtime");
            let git_result = runtime.block_on(GitScanner::scan_repo_async(project.path.clone()));
            let report = DepsParser::detect_and_report(&project.path);
            let mut check = match &git_result {
                Ok(_) => runtime
                    .block_on(registry.check_batch_detailed(&report.dependencies, max_concurrent)),
                Err(_) => BatchCheck {
                    dependencies: report.dependencies,
                    errors: Vec::new(),
                    manifest_errors: report.errors.clone(),
                    non_public_skipped: report.non_public_skipped,
                },
            };
            check.manifest_errors = report.errors;
            check.non_public_skipped = report.non_public_skipped;
            let _ = sender.send(BackgroundEvent::Scan {
                project_id,
                check: check.clone(),
                git_error: git_result.err().map(|error| error.to_string()),
            });
            let reports = runtime
                .block_on(registry.query_vulnerability_batch(&check.dependencies, max_concurrent))
                .into_iter()
                .map(|(package, result)| (package, result.map_err(|error| error.to_string())))
                .collect();
            let _ = sender.send(BackgroundEvent::Vulnerabilities {
                project_id,
                reports,
            });
        });
    }

    pub fn refresh_all(&mut self) {
        let project_ids: Vec<ProjectId> = self
            .config
            .projects
            .iter()
            .map(|project| project.id)
            .collect();
        for project_id in project_ids {
            self.refresh_project(project_id);
        }
    }

    pub fn update_project(&mut self, project_id: ProjectId) {
        let Some(project) = self.project(project_id) else {
            return;
        };
        let dependencies = self
            .dependencies
            .get(&project_id)
            .cloned()
            .unwrap_or_default();
        let plans = dependencies
            .iter()
            .filter(|dependency| dependency.update_available)
            .filter_map(|dependency| self.prepare_update(&project, dependency))
            .collect::<Vec<_>>();
        self.queue_updates(
            plans,
            "No update available for this project",
            UpdateScope::Project,
        );
    }

    pub fn update_dependency(&mut self, project_id: ProjectId, dependency_index: usize) {
        let Some(dependency) = self
            .dependencies
            .get(&project_id)
            .and_then(|dependencies| dependencies.get(dependency_index))
            .cloned()
        else {
            return;
        };
        if !dependency.update_available {
            self.activity
                .push(format!("{} is already up to date", dependency.name));
            return;
        }
        let Some(project) = self.project(project_id) else {
            return;
        };
        let plans = self
            .prepare_update(&project, &dependency)
            .into_iter()
            .collect();
        self.queue_updates(plans, "No update available", UpdateScope::Dependency);
    }

    pub fn update_all_projects(&mut self) {
        let projects = self.config.projects.clone();
        let mut plans = Vec::new();
        for project in projects {
            let dependencies = self
                .dependencies
                .get(&project.id)
                .cloned()
                .unwrap_or_default();
            for dependency in dependencies
                .iter()
                .filter(|dependency| dependency.update_available)
            {
                if let Some(plan) = self.prepare_update(&project, dependency) {
                    plans.push(plan);
                }
            }
        }
        self.queue_updates(plans, "No updates available", UpdateScope::Global);
    }

    pub fn start_pending_updates(&mut self) {
        if self.pending_updates.is_empty() || self.updating {
            return;
        }
        self.modal = None;
        self.updating = true;
        self.cancel_update.store(false, Ordering::Relaxed);
        let pending = self.pending_updates.clone();
        let total = pending.len();
        let trusted: HashSet<PathBuf> = self
            .config
            .projects
            .iter()
            .filter(|project| self.config.trusted_repositories.contains(&project.id))
            .map(|project| project.path.clone())
            .collect();
        let backup_root = self.backup_root.join("updates");
        let lock_root = self.backup_root.join("locks");
        let sender = self.background_sender.clone();
        let cancel = self.cancel_update.clone();
        std::thread::spawn(move || {
            let groups = group_updates(pending);
            let mut touched = Vec::new();
            let mut succeeded = 0;
            let mut failed = 0;
            for (project_id, expected_head, plans) in groups {
                if cancel.load(Ordering::Relaxed) {
                    failed += plans.len();
                    continue;
                }
                let project_path = plans[0].project_path.clone();
                let trusted_for_scripts = trusted.contains(&project_path);
                let outcome = run_project_updates(
                    project_id,
                    &project_path,
                    &expected_head,
                    &plans,
                    trusted_for_scripts,
                    &backup_root,
                    &lock_root,
                    &cancel,
                    &sender,
                );
                touched.push(project_id);
                succeeded += outcome.succeeded;
                failed += outcome.failed;
                if let Some(message) = outcome.message {
                    let _ = sender.send(BackgroundEvent::ProjectFinished {
                        project_id,
                        message,
                    });
                }
            }
            let summary = if cancel.load(Ordering::Relaxed) {
                format!(
                    "Updates cancelled: {succeeded} updated, {} failed or skipped",
                    total.saturating_sub(succeeded)
                )
            } else {
                format!("Updates finished: {succeeded} updated, {failed} failed or skipped")
            };
            let _ = sender.send(BackgroundEvent::UpdatesFinished {
                summary,
                projects: touched,
            });
        });
    }

    pub fn cancel_updates(&mut self) {
        self.cancel_update.store(true, Ordering::Relaxed);
    }

    pub fn is_updating(&self) -> bool {
        self.updating
    }

    pub fn process_update_events(&mut self) {
        while let Ok(event) = self.background_receiver.try_recv() {
            match event {
                BackgroundEvent::Scan {
                    project_id,
                    check,
                    git_error,
                } => {
                    self.scanning.remove(&project_id);
                    if let Some(error) = git_error {
                        self.git_errors.insert(project_id, error);
                    } else {
                        self.git_errors.remove(&project_id);
                    }
                    if check.non_public_skipped {
                        self.non_public_projects.insert(project_id);
                    } else {
                        self.non_public_projects.remove(&project_id);
                    }
                    self.registry_errors
                        .insert(project_id, check.errors.clone());
                    self.manifest_errors
                        .insert(project_id, check.manifest_errors.clone());
                    self.dependencies
                        .insert(project_id, check.dependencies.clone());
                }
                BackgroundEvent::Vulnerabilities {
                    project_id,
                    reports,
                } => {
                    self.auditing.remove(&project_id);
                    for (package, report) in reports {
                        self.vulnerabilities.insert((project_id, package), report);
                    }
                }
                BackgroundEvent::Update {
                    project_id,
                    package,
                    result,
                } => {
                    let project_name = self
                        .project(project_id)
                        .map(|project| project.name)
                        .unwrap_or_else(|| "project".into());
                    match result {
                        Ok(output) => self
                            .activity
                            .push(format!("Updated {package} in {project_name}: {output}")),
                        Err(error) => self.activity.push(format!(
                            "Update failed for {package} in {project_name}: {error}"
                        )),
                    }
                }
                BackgroundEvent::ProjectFinished {
                    project_id,
                    message,
                } => {
                    let project_name = self
                        .project(project_id)
                        .map(|project| project.name)
                        .unwrap_or_else(|| "Project".into());
                    self.activity.push(format!("{project_name}: {message}"));
                }
                BackgroundEvent::UpdatesFinished { summary, projects } => {
                    self.updating = false;
                    self.pending_updates.clear();
                    self.activity.push(summary);
                    for project_id in projects {
                        self.refresh_project(project_id);
                    }
                }
            }
        }
        self.trim_activity();
    }

    fn prepare_update(
        &mut self,
        project: &Project,
        dependency: &crate::core::DepInfo,
    ) -> Option<PendingUpdate> {
        let expected_head = match head(&project.path) {
            Ok(head) => head,
            Err(error) => {
                self.activity
                    .push(format!("Cannot prepare {}: {error}", dependency.name));
                return None;
            }
        };
        match UpdateManager::plan(&project.path, dependency) {
            Ok(plan) => Some(PendingUpdate {
                project_id: project.id,
                plan,
                expected_head,
            }),
            Err(error) => {
                self.activity
                    .push(format!("Cannot prepare {}: {error}", dependency.name));
                None
            }
        }
    }

    fn queue_updates(
        &mut self,
        plans: Vec<PendingUpdate>,
        empty_message: &str,
        scope: UpdateScope,
    ) {
        self.pending_updates = plans;
        self.pending_scope = scope;
        if self.pending_updates.is_empty() {
            self.activity.push(empty_message.into());
        } else {
            self.modal = Some(DesktopModal::UpdateConfirmation);
        }
    }

    fn project(&self, project_id: ProjectId) -> Option<Project> {
        self.config
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
    }
}

struct ProjectRunOutcome {
    succeeded: usize,
    failed: usize,
    message: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn run_project_updates(
    project_id: ProjectId,
    project_path: &Path,
    expected_head: &str,
    plans: &[UpdatePlan],
    trusted_for_scripts: bool,
    backup_root: &Path,
    lock_root: &Path,
    cancel: &Arc<AtomicBool>,
    sender: &std::sync::mpsc::Sender<BackgroundEvent>,
) -> ProjectRunOutcome {
    let result = (|| -> Result<(usize, Option<String>)> {
        let _lock = ProjectLock::acquire(&lock_root.join(format!("{project_id}.lock")))?;
        assert_clean_at(project_path, expected_head)?;
        let mut backup = UpdateBackup::create(project_path, backup_root)?;
        let backup_directory = backup.directory().to_path_buf();
        let mut succeeded = 0;
        let mut failure = None;
        for plan in plans {
            let result = if cancel.load(Ordering::Relaxed) {
                Err(AppError::Other("Update cancelled by user".into()))
            } else {
                UpdateManager::validate_plan(plan).and_then(|_| {
                    UpdateManager::execute_with_policy(
                        plan,
                        Some(cancel.clone()),
                        trusted_for_scripts,
                    )
                })
            };
            match result {
                Ok(outcome) => {
                    succeeded += 1;
                    let _ = sender.send(BackgroundEvent::Update {
                        project_id,
                        package: plan.package.clone(),
                        result: Ok(outcome.output),
                    });
                }
                Err(error) => {
                    failure = Some(error.to_string());
                    let _ = sender.send(BackgroundEvent::Update {
                        project_id,
                        package: plan.package.clone(),
                        result: Err(failure.clone().unwrap()),
                    });
                    break;
                }
            }
        }
        if failure.is_some() {
            backup.capture_after(project_path)?;
            let recovery = backup
                .rollback_if_unchanged(project_path)
                .map(|directory| {
                    format!(
                        "Project changes rolled back safely; backup retained at {}",
                        directory.display()
                    )
                })
                .unwrap_or_else(|rollback| format!("Automatic rollback unavailable: {rollback}"));
            return Ok((succeeded, Some(recovery)));
        }
        Ok((
            succeeded,
            Some(format!(
                "Project updated; manifest backup retained at {}",
                backup_directory.display()
            )),
        ))
    })();
    match result {
        Ok((succeeded, message)) => ProjectRunOutcome {
            succeeded,
            failed: plans.len().saturating_sub(succeeded),
            message,
        },
        Err(error) => ProjectRunOutcome {
            succeeded: 0,
            failed: plans.len(),
            message: Some(format!("Project update refused: {error}")),
        },
    }
}

fn group_updates(pending: Vec<PendingUpdate>) -> Vec<(ProjectId, String, Vec<UpdatePlan>)> {
    let mut groups: Vec<(ProjectId, String, Vec<UpdatePlan>)> = Vec::new();
    for update in pending {
        if let Some(group) = groups
            .iter_mut()
            .find(|(project_id, _, _)| *project_id == update.project_id)
        {
            group.2.push(update.plan);
        } else {
            groups.push((update.project_id, update.expected_head, vec![update.plan]));
        }
    }
    groups
}

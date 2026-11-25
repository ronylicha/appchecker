use super::ai::{AiProvider, AnalysisResponse, AppliedPatch, ModelInfo, ProviderStatus};
use super::remediation::AiEvent;
use super::theme::{DesktopView, Theme};
use super::updates::{BackgroundEvent, PendingUpdate};
use super::widgets::{
    card_title, danger_button, muted_label, navigation_button, page_title, pill_badge,
    primary_action, primary_button, secondary_button, section_heading,
};
use crate::config::{ConfigLoader, ProjectDraft, ProjectManager};
use crate::core::{AppConfig, DepInfo, Project, ProjectId, Result};
use crate::deps::{CheckIssue, ManifestError, Vulnerability};
use crate::persist::Store;
use eframe::egui::{CentralPanel, RichText, Widget};
use egui_extras::{Column, TableBuilder};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UpdateScope {
    Dependency,
    Project,
    Global,
}

impl UpdateScope {
    fn title(self) -> &'static str {
        match self {
            Self::Dependency => "Confirm dependency update",
            Self::Project => "Confirm project updates",
            Self::Global => "Confirm updates across all projects",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DesktopModal {
    AddProject,
    EditProject(ProjectId),
    RemoveProject(ProjectId),
    Discovery,
    UpdateConfirmation,
    ApplyPatch,
    RollbackPatch,
}

pub struct DesktopApp {
    view: DesktopView,
    pub(crate) theme: Theme,
    pub(crate) config: AppConfig,
    config_path: PathBuf,
    pub(crate) backup_root: PathBuf,
    store: Store,
    pub(crate) selected_project: Option<ProjectId>,
    pub(crate) modal: Option<DesktopModal>,
    project_path: String,
    project_name: String,
    project_tags: String,
    project_pinned: bool,
    project_trusted: bool,
    project_error: Option<String>,
    project_search: String,
    updates_only: bool,
    discovered_paths: Option<Vec<PathBuf>>,
    discovery_receiver: Option<std::sync::mpsc::Receiver<Vec<PathBuf>>>,
    directory_receiver: Option<std::sync::mpsc::Receiver<PathBuf>>,
    cancel_background: Arc<AtomicBool>,
    pub(crate) background_sender: std::sync::mpsc::Sender<BackgroundEvent>,
    pub(crate) background_receiver: std::sync::mpsc::Receiver<BackgroundEvent>,
    pub(crate) dependencies: HashMap<ProjectId, Vec<DepInfo>>,
    pub(crate) scanning: HashSet<ProjectId>,
    pub(crate) auditing: HashSet<ProjectId>,
    pub(crate) vulnerabilities:
        HashMap<(ProjectId, String), std::result::Result<Vec<Vulnerability>, String>>,
    pub(crate) updating: bool,
    pub(crate) pending_updates: Vec<PendingUpdate>,
    pub(crate) pending_scope: UpdateScope,
    pub(crate) cancel_update: Arc<AtomicBool>,
    pub(crate) git_errors: HashMap<ProjectId, String>,
    pub(crate) registry_errors: HashMap<ProjectId, Vec<CheckIssue>>,
    pub(crate) manifest_errors: HashMap<ProjectId, Vec<ManifestError>>,
    pub(crate) non_public_projects: HashSet<ProjectId>,
    pub(crate) activity: Vec<String>,
    pub(crate) ai_provider: AiProvider,
    pub(crate) ai_status: Option<ProviderStatus>,
    pub(crate) ai_models: Vec<ModelInfo>,
    pub(crate) ai_model: Option<String>,
    pub(crate) antigravity_terms_accepted: bool,
    pub(crate) ai_status_busy: bool,
    pub(crate) ai_busy: bool,
    pub(crate) ai_files: Vec<PathBuf>,
    pub(crate) ai_consent: bool,
    pub(crate) ai_analysis: Option<AnalysisResponse>,
    pub(crate) ai_applied: Option<AppliedPatch>,
    pub(crate) ai_error: Option<String>,
    pub(crate) ai_notice: Option<String>,
    pub(crate) ai_sender: std::sync::mpsc::Sender<AiEvent>,
    pub(crate) ai_receiver: std::sync::mpsc::Receiver<AiEvent>,
}

impl DesktopApp {
    pub fn new(config: AppConfig, config_path: PathBuf, store: Store) -> Self {
        let (background_sender, background_receiver) = std::sync::mpsc::channel();
        let (ai_sender, ai_receiver) = std::sync::mpsc::channel();
        let backup_root = config_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("backups");
        let dependencies = config
            .projects
            .iter()
            .filter_map(|project| {
                store
                    .get_deps(project.id)
                    .ok()
                    .map(|deps| (project.id, deps))
            })
            .collect();
        let selected_project = config.projects.first().map(|project| project.id);
        Self {
            view: DesktopView::Projects,
            theme: Theme::Light,
            config,
            config_path,
            backup_root,
            store,
            selected_project,
            modal: None,
            project_path: String::new(),
            project_name: String::new(),
            project_tags: String::new(),
            project_pinned: false,
            project_trusted: false,
            project_error: None,
            project_search: String::new(),
            updates_only: false,
            discovered_paths: None,
            discovery_receiver: None,
            directory_receiver: None,
            cancel_background: Arc::new(AtomicBool::new(false)),
            background_sender,
            background_receiver,
            dependencies,
            scanning: HashSet::new(),
            auditing: HashSet::new(),
            vulnerabilities: HashMap::new(),
            updating: false,
            pending_updates: Vec::new(),
            pending_scope: UpdateScope::Project,
            cancel_update: Arc::new(AtomicBool::new(false)),
            git_errors: HashMap::new(),
            registry_errors: HashMap::new(),
            manifest_errors: HashMap::new(),
            non_public_projects: HashSet::new(),
            activity: Vec::new(),
            ai_provider: AiProvider::Claude,
            ai_status: None,
            ai_models: Vec::new(),
            ai_model: None,
            antigravity_terms_accepted: false,
            ai_status_busy: false,
            ai_busy: false,
            ai_files: Vec::new(),
            ai_consent: false,
            ai_analysis: None,
            ai_applied: None,
            ai_error: None,
            ai_notice: None,
            ai_sender,
            ai_receiver,
        }
    }

    pub fn projects(&self) -> &[Project] {
        &self.config.projects
    }

    pub fn add_project(&mut self, draft: ProjectDraft) -> Result<()> {
        self.save_project(None, draft)
    }

    pub fn remove_project(&mut self, project_id: ProjectId) -> Result<()> {
        let previous = self.config.clone();
        let project = ProjectManager::remove(&mut self.config, project_id)?;
        self.config
            .trusted_repositories
            .retain(|id| *id != project_id);
        if let Err(error) = ConfigLoader::save(&self.config, &self.config_path) {
            self.config = previous;
            return Err(error);
        }
        self.store.remove_project(project_id)?;
        self.dependencies.remove(&project_id);
        self.git_errors.remove(&project_id);
        self.registry_errors.remove(&project_id);
        self.manifest_errors.remove(&project_id);
        self.non_public_projects.remove(&project_id);
        self.auditing.remove(&project_id);
        self.vulnerabilities.retain(|(id, _), _| *id != project_id);
        if self.selected_project == Some(project_id) {
            self.selected_project = None;
        }
        self.modal = None;
        self.activity.push(format!("Removed {}", project.name));
        Ok(())
    }

    pub fn toggle_pin(&mut self, project_id: ProjectId) -> Result<()> {
        let previous = self.config.clone();
        let pinned = ProjectManager::toggle_pin(&mut self.config, project_id)?;
        if let Err(error) = ConfigLoader::save(&self.config, &self.config_path) {
            self.config = previous;
            return Err(error);
        }
        if let Some(project) = self
            .config
            .projects
            .iter()
            .find(|project| project.id == project_id)
        {
            self.store.upsert_project(project)?;
            self.activity.push(format!(
                "{} {}",
                project.name,
                if pinned { "pinned" } else { "unpinned" }
            ));
        }
        Ok(())
    }

    pub fn set_project_trusted(&mut self, project_id: ProjectId, trusted: bool) -> Result<()> {
        let previous = self.config.clone();
        if trusted {
            if !self.config.trusted_repositories.contains(&project_id) {
                self.config.trusted_repositories.push(project_id);
            }
        } else {
            self.config
                .trusted_repositories
                .retain(|id| *id != project_id);
        }
        if let Err(error) = ConfigLoader::save(&self.config, &self.config_path) {
            self.config = previous;
            return Err(error);
        }
        self.project_trusted = trusted;
        Ok(())
    }

    fn save_project(&mut self, project_id: Option<ProjectId>, draft: ProjectDraft) -> Result<()> {
        let previous = self.config.clone();
        let result = match project_id {
            Some(project_id) => ProjectManager::update(&mut self.config, project_id, draft),
            None => ProjectManager::add(&mut self.config, draft),
        };
        let project = result?;
        if self.project_trusted {
            if !self.config.trusted_repositories.contains(&project.id) {
                self.config.trusted_repositories.push(project.id);
            }
        } else {
            self.config
                .trusted_repositories
                .retain(|id| *id != project.id);
        }
        if let Err(error) = ConfigLoader::save(&self.config, &self.config_path) {
            self.config = previous;
            return Err(error);
        }
        self.store.upsert_project(&project)?;
        self.select_project(project.id);
        self.modal = None;
        self.project_error = None;
        self.activity.push(format!("Saved {}", project.name));
        self.refresh_project(project.id);
        Ok(())
    }

    fn open_add_project(&mut self) {
        self.project_path.clear();
        self.project_name.clear();
        self.project_tags.clear();
        self.project_pinned = false;
        self.project_trusted = false;
        self.project_error = None;
        self.modal = Some(DesktopModal::AddProject);
    }

    fn open_edit_project(&mut self, project_id: ProjectId) {
        if let Some(project) = self
            .config
            .projects
            .iter()
            .find(|project| project.id == project_id)
        {
            self.project_path = project.path.display().to_string();
            self.project_name = project.name.clone();
            self.project_tags = project.tags.join(", ");
            self.project_pinned = project.pinned;
            self.project_trusted = self.config.trusted_repositories.contains(&project_id);
            self.project_error = None;
            self.modal = Some(DesktopModal::EditProject(project_id));
        }
    }

    pub(crate) fn select_project(&mut self, project_id: ProjectId) {
        if self.selected_project == Some(project_id) {
            return;
        }
        self.selected_project = Some(project_id);
        self.ai_files.clear();
        self.ai_consent = false;
        self.ai_analysis = None;
        self.ai_applied = None;
        self.ai_error = None;
        self.ai_notice = None;
    }

    fn project_draft(&self) -> ProjectDraft {
        ProjectDraft {
            path: self.project_path.trim().into(),
            name: self.project_name.trim().to_string(),
            tags: self
                .project_tags
                .split(',')
                .map(|tag| tag.trim().to_string())
                .filter(|tag| !tag.is_empty())
                .collect(),
            pinned: self.project_pinned,
        }
    }

    fn discover_projects(&mut self) {
        let paths = self.config.auto_discover_paths.clone();
        let depth = self.config.max_auto_discover_depth;
        let existing: Vec<PathBuf> = self
            .config
            .projects
            .iter()
            .map(|project| project.path.clone())
            .collect();
        let (sender, receiver) = std::sync::mpsc::channel();
        self.discovery_receiver = Some(receiver);
        let cancel = self.cancel_background.clone();
        std::thread::spawn(move || {
            let paths = ConfigLoader::auto_discover_with_cancel(&paths, depth, &existing, cancel);
            let _ = sender.send(paths);
        });
    }

    fn add_discovered(&mut self) -> Result<()> {
        let Some(paths) = self.discovered_paths.clone() else {
            return Ok(());
        };
        let previous = self.config.clone();
        let mut added = Vec::new();
        for path in paths {
            if let Ok(project) = ProjectManager::add(
                &mut self.config,
                ProjectDraft {
                    path,
                    name: String::new(),
                    tags: Vec::new(),
                    pinned: false,
                },
            ) {
                added.push(project);
            }
        }
        if let Err(error) = ConfigLoader::save(&self.config, &self.config_path) {
            self.config = previous;
            return Err(error);
        }
        for project in &added {
            self.store.upsert_project(project)?;
        }
        self.discovered_paths = None;
        self.modal = None;
        self.activity
            .push(format!("Added {} discovered project(s)", added.len()));
        Ok(())
    }

    pub(crate) fn trim_activity(&mut self) {
        const MAX_ACTIVITY: usize = 500;
        if self.activity.len() > MAX_ACTIVITY {
            self.activity.drain(..self.activity.len() - MAX_ACTIVITY);
        }
    }

    pub(crate) fn project_update_count(&self, project_id: ProjectId) -> usize {
        self.dependencies
            .get(&project_id)
            .map(|dependencies| {
                dependencies
                    .iter()
                    .filter(|dependency| dependency.update_available)
                    .count()
            })
            .unwrap_or(0)
    }

    fn total_update_count(&self) -> usize {
        self.config
            .projects
            .iter()
            .map(|project| self.project_update_count(project.id))
            .sum()
    }

    fn has_background_work(&self) -> bool {
        !self.scanning.is_empty()
            || !self.auditing.is_empty()
            || self.updating
            || self.ai_busy
            || self.ai_status_busy
            || self.directory_receiver.is_some()
            || self.discovery_receiver.is_some()
    }

    fn process_background(&mut self) {
        self.process_update_events();
        self.process_ai_events();
        if let Some(receiver) = &self.directory_receiver
            && let Ok(path) = receiver.try_recv()
        {
            self.project_path = path.display().to_string();
            self.directory_receiver = None;
        }
        if let Some(receiver) = &self.discovery_receiver
            && let Ok(paths) = receiver.try_recv()
        {
            self.discovered_paths = Some(paths);
            self.discovery_receiver = None;
            self.modal = Some(DesktopModal::Discovery);
        }
    }
}

impl eframe::App for DesktopApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.process_background();
        let (requested_view, add_project) = root.ctx().input_mut(|input| {
            (
                [
                    (egui::Key::Num1, DesktopView::Projects),
                    (egui::Key::Num2, DesktopView::Updates),
                    (egui::Key::Num3, DesktopView::Remediation),
                    (egui::Key::Num4, DesktopView::Activity),
                ]
                .into_iter()
                .find(|(key, _)| input.consume_key(egui::Modifiers::CTRL, *key))
                .map(|(_, view)| view),
                input.consume_key(egui::Modifiers::CTRL, egui::Key::N),
            )
        });
        if let Some(view) = requested_view {
            self.view = view;
        }
        if add_project {
            self.open_add_project();
        }
        let style = self.theme.text_style();
        root.ctx().set_style_of(egui::Theme::Light, style.clone());
        root.ctx().set_style_of(egui::Theme::Dark, style);
        if self.has_background_work() {
            root.ctx()
                .request_repaint_after(std::time::Duration::from_millis(80));
        }
        let tokens = self.theme.tokens();

        egui::Panel::top("app-header")
            .frame(egui::Frame::new().fill(tokens.surface).inner_margin(16.0))
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("AppChecker")
                            .size(25.0)
                            .strong()
                            .color(tokens.text),
                    );
                    ui.separator();
                    muted_label(ui, "Local project health", tokens);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let label = if self.theme == Theme::Light {
                            "Dark theme"
                        } else {
                            "Light theme"
                        };
                        if secondary_button(label, tokens).ui(ui).clicked() {
                            self.theme = match self.theme {
                                Theme::Light => Theme::Dark,
                                Theme::Dark => Theme::Light,
                            };
                        }
                    });
                });
            });

        egui::Panel::left("navigation")
            .resizable(false)
            .default_size(200.0)
            .frame(
                egui::Frame::new()
                    .fill(tokens.background)
                    .inner_margin(12.0),
            )
            .show(root, |ui| {
                ui.add_space(8.0);
                for view in DesktopView::ALL {
                    if navigation_button(view, self.view == view, tokens)
                        .ui(ui)
                        .clicked()
                    {
                        self.view = view;
                    }
                    ui.add_space(6.0);
                }
            });

        if self.updating {
            egui::Panel::bottom("update-progress")
                .frame(egui::Frame::new().fill(tokens.selection).inner_margin(12.0))
                .show(root, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Updates are running")
                                .color(tokens.text)
                                .strong(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if secondary_button("Cancel updates", tokens).ui(ui).clicked() {
                                self.cancel_updates();
                            }
                        });
                    });
                });
        }

        CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(tokens.background)
                    .inner_margin(24.0),
            )
            .show(root, |ui| {
                ui.set_max_width(1120.0);
                match self.view {
                    DesktopView::Projects => self.projects_view(ui),
                    DesktopView::Updates => self.updates_view(ui),
                    DesktopView::Remediation => self.remediation_view(ui),
                    DesktopView::Activity => self.activity_view(ui),
                }
            });

        self.show_active_modal(root);
    }
}

impl DesktopApp {
    fn projects_view(&mut self, ui: &mut egui::Ui) {
        let tokens = self.theme.tokens();
        page_title(ui, "Projects", tokens);
        muted_label(
            ui,
            "Monitor Git repositories, understand their health, and choose exactly what to update.",
            tokens,
        );
        ui.add_space(18.0);
        ui.horizontal_wrapped(|ui| {
            if primary_button("+ Add project", tokens).ui(ui).clicked() {
                self.open_add_project();
            }
            if secondary_button("Discover", tokens).ui(ui).clicked() {
                self.discover_projects();
            }
            if secondary_button("Refresh all", tokens).ui(ui).clicked() {
                self.refresh_all();
            }
        });
        ui.add_space(10.0);
        let total = self.total_update_count();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    total > 0 && !self.updating,
                    primary_action(
                        total > 0 && !self.updating,
                        format!("Update all projects ({total})"),
                        tokens,
                    ),
                )
                .clicked()
            {
                self.update_all_projects();
            }
        });
        ui.add_space(12.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.project_search)
                .hint_text("Search projects or tags…")
                .desired_width(360.0),
        );
        ui.add_space(16.0);
        if self.config.projects.is_empty() {
            self.empty_state(
                ui,
                "No projects monitored yet",
                "Add a Git repository with the native folder picker to get started.",
            );
            return;
        }
        let query = self.project_search.to_ascii_lowercase();
        let projects: Vec<Project> = self
            .config
            .projects
            .iter()
            .filter(|project| {
                query.is_empty()
                    || project.name.to_ascii_lowercase().contains(&query)
                    || project
                        .tags
                        .iter()
                        .any(|tag| tag.to_ascii_lowercase().contains(&query))
            })
            .cloned()
            .collect();
        if projects.is_empty() {
            muted_label(ui, "No projects match this search.", tokens);
            return;
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for project in projects {
                self.project_card(ui, project);
                ui.add_space(10.0);
            }
        });
    }

    fn project_card(&mut self, ui: &mut egui::Ui, project: Project) {
        let tokens = self.theme.tokens();
        let selected = self.selected_project == Some(project.id);
        let updates = self.project_update_count(project.id);
        egui::Frame::group(ui.style())
            .fill(if selected {
                tokens.selection
            } else {
                tokens.surface
            })
            .stroke((
                if selected { 2.0 } else { 1.0 },
                if selected {
                    tokens.accent
                } else {
                    tokens.border
                },
            ))
            .corner_radius(10.0)
            .inner_margin(14.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    card_title(ui, &project.name, tokens);
                    if project.pinned {
                        pill_badge(ui, "Pinned", tokens.accent_text, tokens);
                    }
                    if self.scanning.contains(&project.id) {
                        pill_badge(ui, "Scanning…", tokens.warning, tokens);
                    } else if updates > 0 {
                        pill_badge(ui, &format!("{updates} updates"), tokens.warning, tokens);
                    } else if self.git_errors.contains_key(&project.id) {
                        pill_badge(ui, "Git error", tokens.error, tokens);
                    } else {
                        pill_badge(ui, "Up to date", tokens.success, tokens);
                    }
                    for tag in &project.tags {
                        pill_badge(ui, tag, tokens.muted_text, tokens);
                    }
                });
                ui.add_space(6.0);
                let path = project.path.display().to_string();
                ui.add(egui::Label::new(RichText::new(&path).color(tokens.muted_text)).truncate())
                    .on_hover_text(path);
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if secondary_button("Select", tokens).ui(ui).clicked() {
                        self.select_project(project.id);
                        self.view = DesktopView::Updates;
                    }
                    if ui
                        .add_enabled(
                            updates > 0 && !self.updating,
                            secondary_button(format!("Update project ({updates})"), tokens),
                        )
                        .clicked()
                    {
                        self.update_project(project.id);
                    }
                    if secondary_button("Edit", tokens).ui(ui).clicked() {
                        self.open_edit_project(project.id);
                    }
                    let pin_label = if project.pinned { "Unpin" } else { "Pin" };
                    if secondary_button(pin_label, tokens).ui(ui).clicked()
                        && let Err(error) = self.toggle_pin(project.id)
                    {
                        self.activity.push(error.to_string());
                    }
                    if danger_button("Remove", tokens).ui(ui).clicked() {
                        self.modal = Some(DesktopModal::RemoveProject(project.id));
                    }
                });
            });
    }

    fn updates_view(&mut self, ui: &mut egui::Ui) {
        let tokens = self.theme.tokens();
        page_title(ui, "Dependency updates", tokens);
        muted_label(
            ui,
            "Review registry results and choose a single dependency or the whole project.",
            tokens,
        );
        ui.add_space(14.0);
        self.project_switcher(ui);
        let Some(project_id) = self.selected_project else {
            muted_label(
                ui,
                "Select a project in Projects to review dependencies.",
                tokens,
            );
            return;
        };
        let count = self.project_update_count(project_id);
        ui.horizontal_wrapped(|ui| {
            if secondary_button("Refresh project", tokens).ui(ui).clicked() {
                self.refresh_project(project_id);
            }
            if ui
                .add_enabled(
                    count > 0 && !self.updating,
                    primary_action(
                        count > 0 && !self.updating,
                        format!("Update project ({count})"),
                        tokens,
                    ),
                )
                .clicked()
            {
                self.update_project(project_id);
            }
            ui.checkbox(&mut self.updates_only, "Updates only");
        });
        ui.add_space(10.0);
        self.scan_errors(ui, project_id);
        let dependencies: Vec<(usize, DepInfo)> = self
            .dependencies
            .get(&project_id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .filter(|(_, dependency)| !self.updates_only || dependency.update_available)
            .collect();
        if dependencies.is_empty() {
            if self.non_public_projects.contains(&project_id) {
                muted_label(
                    ui,
                    "Private/local dependencies skipped: no public registry checks were performed.",
                    tokens,
                );
            } else if self.scanning.contains(&project_id) {
                ui.horizontal(|ui| {
                    ui.spinner();
                    muted_label(ui, "Checking dependency manifests…", tokens);
                });
            } else if self.updates_only {
                muted_label(ui, "No actionable updates match this filter.", tokens);
            } else {
                muted_label(ui, "No supported public dependency manifest found.", tokens);
            }
            return;
        }
        egui::ScrollArea::horizontal().show(ui, |ui| {
            ui.set_min_width(980.0);
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::auto().at_least(150.0))
                .column(Column::auto().at_least(80.0))
                .column(Column::auto().at_least(100.0))
                .column(Column::auto().at_least(100.0))
                .column(Column::auto().at_least(90.0))
                .column(Column::auto().at_least(190.0))
                .column(Column::auto().at_least(100.0))
                .header(32.0, |mut header| {
                    for label in [
                        "Package",
                        "Ecosystem",
                        "Current",
                        "Latest",
                        "Update level",
                        "CVE audit",
                        "Action",
                    ] {
                        header.col(|ui| {
                            ui.label(RichText::new(label).strong().color(tokens.text));
                        });
                    }
                })
                .body(|body| {
                    body.rows(58.0, dependencies.len(), |mut row| {
                        let (index, dependency) = &dependencies[row.index()];
                        row.col(|ui| {
                            card_title(ui, &dependency.name, tokens);
                        });
                        row.col(|ui| {
                            muted_label(ui, &format!("{:?}", dependency.ecosystem), tokens);
                        });
                        row.col(|ui| {
                            ui.label(RichText::new(&dependency.current_version).color(tokens.text));
                        });
                        row.col(|ui| {
                            ui.label(
                                RichText::new(
                                    dependency
                                        .latest_version
                                        .as_deref()
                                        .unwrap_or("Not checked"),
                                )
                                .color(tokens.text),
                            );
                        });
                        row.col(|ui| {
                            let color = match dependency.severity {
                                crate::core::UpdateSeverity::Security => tokens.error,
                                crate::core::UpdateSeverity::Major => tokens.warning,
                                _ => tokens.muted_text,
                            };
                            pill_badge(ui, &dependency.severity.to_string(), color, tokens);
                        });
                        row.col(|ui| {
                            self.vulnerability_cell(ui, project_id, dependency);
                        });
                        row.col(|ui| {
                            if dependency.update_available {
                                if ui
                                    .add_enabled(!self.updating, secondary_button("Update", tokens))
                                    .clicked()
                                {
                                    self.update_dependency(project_id, *index);
                                }
                            } else if dependency.latest_version.is_some() {
                                pill_badge(ui, "Current", tokens.success, tokens);
                            } else {
                                pill_badge(ui, "No action", tokens.muted_text, tokens);
                            }
                        });
                    });
                });
        });
    }

    fn vulnerability_cell(&self, ui: &mut egui::Ui, project_id: ProjectId, dependency: &DepInfo) {
        let tokens = self.theme.tokens();
        if self.auditing.contains(&project_id) {
            muted_label(ui, "Checking OSV…", tokens);
            return;
        }
        let Some(report) = self
            .vulnerabilities
            .get(&(project_id, dependency.name.clone()))
        else {
            muted_label(ui, "Not checked", tokens);
            return;
        };
        match report {
            Ok(vulnerabilities) if vulnerabilities.is_empty() => {
                pill_badge(ui, "No known CVE", tokens.success, tokens)
                    .on_hover_text("OSV returned no vulnerability for this exact version");
            }
            Ok(vulnerabilities) => {
                let severity = crate::deps::highest_vulnerability_severity(vulnerabilities);
                let color = match severity {
                    crate::deps::VulnerabilitySeverity::Critical
                    | crate::deps::VulnerabilitySeverity::High => tokens.error,
                    crate::deps::VulnerabilitySeverity::Medium => tokens.warning,
                    _ => tokens.muted_text,
                };
                let aliases: Vec<String> = vulnerabilities
                    .iter()
                    .flat_map(|vulnerability| vulnerability.aliases.clone())
                    .filter(|alias| alias.starts_with("CVE-"))
                    .collect();
                let identifiers = if aliases.is_empty() {
                    vulnerabilities
                        .iter()
                        .map(|vulnerability| vulnerability.id.clone())
                        .collect::<Vec<_>>()
                } else {
                    aliases
                };
                let remaining = identifiers.len().saturating_sub(2);
                let mut display = identifiers
                    .into_iter()
                    .take(2)
                    .collect::<Vec<_>>()
                    .join(", ");
                if remaining > 0 {
                    display.push_str(&format!(" +{remaining}"));
                }
                let label = format!("{severity} · {display}");
                pill_badge(ui, &label, color, tokens).on_hover_ui(|ui| {
                    for vulnerability in vulnerabilities {
                        ui.label(RichText::new(&vulnerability.id).strong().color(tokens.text));
                        if !vulnerability.summary.is_empty() {
                            ui.label(&vulnerability.summary);
                        }
                        ui.add_space(6.0);
                    }
                });
            }
            Err(error) => {
                pill_badge(ui, "CVE unavailable", tokens.warning, tokens).on_hover_text(error);
            }
        }
    }

    pub(crate) fn project_switcher(&mut self, ui: &mut egui::Ui) {
        let selected_name = self
            .selected_project
            .and_then(|id| self.config.projects.iter().find(|project| project.id == id))
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "Choose a project".into());
        let projects = self.config.projects.clone();
        egui::ComboBox::from_label("Project")
            .selected_text(selected_name)
            .show_ui(ui, |ui| {
                for project in projects {
                    if ui
                        .selectable_label(self.selected_project == Some(project.id), &project.name)
                        .clicked()
                    {
                        self.select_project(project.id);
                    }
                }
            });
    }

    fn scan_errors(&self, ui: &mut egui::Ui, project_id: ProjectId) {
        let tokens = self.theme.tokens();
        let mut messages = Vec::new();
        if let Some(error) = self.git_errors.get(&project_id) {
            messages.push(format!("Git: {error}"));
        }
        for error in self.registry_errors.get(&project_id).into_iter().flatten() {
            messages.push(format!("Registry {}: {}", error.package, error.message));
        }
        for error in self.manifest_errors.get(&project_id).into_iter().flatten() {
            messages.push(format!("Manifest: {error}"));
        }
        if !messages.is_empty() {
            egui::Frame::group(ui.style())
                .fill(tokens.surface)
                .stroke((1.0, tokens.error))
                .corner_radius(8.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Checks need attention")
                            .color(tokens.error)
                            .strong(),
                    );
                    for message in messages {
                        ui.label(RichText::new(message).color(tokens.text));
                    }
                });
            ui.add_space(10.0);
        }
    }

    fn activity_view(&mut self, ui: &mut egui::Ui) {
        let tokens = self.theme.tokens();
        page_title(ui, "Activity", tokens);
        muted_label(ui, "Sanitized operations from this session only.", tokens);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if secondary_button("Copy", tokens).ui(ui).clicked() {
                ui.ctx().copy_text(self.activity.join("\n"));
            }
            if secondary_button("Clear", tokens).ui(ui).clicked() {
                self.activity.clear();
            }
        });
        ui.add_space(12.0);
        if self.activity.is_empty() {
            self.empty_state(
                ui,
                "No activity yet",
                "Scans, updates and remediation results will appear here.",
            );
            return;
        }
        let entries: Vec<String> = self.activity.iter().rev().cloned().collect();
        egui::ScrollArea::vertical().show_rows(ui, 34.0, entries.len(), |ui, rows| {
            for row in rows {
                let entry = &entries[row];
                let color = if entry.contains("failed") || entry.contains("refused") {
                    tokens.error
                } else if entry.contains("cancelled") || entry.contains("skipped") {
                    tokens.warning
                } else {
                    tokens.text
                };
                ui.label(RichText::new(entry).color(color));
            }
        });
    }

    fn empty_state(&self, ui: &mut egui::Ui, title: &str, detail: &str) {
        let tokens = self.theme.tokens();
        ui.add_space(32.0);
        section_heading(ui, title, tokens);
        muted_label(ui, detail, tokens);
    }

    fn show_active_modal(&mut self, root: &mut egui::Ui) {
        let Some(modal) = self.modal else {
            return;
        };
        let id = egui::Id::unique(format!("modal-{modal:?}"));
        let response = egui::Modal::new(id)
            .frame(egui::Frame::window(root.style()))
            .show(root.ctx(), |ui| match modal {
                DesktopModal::AddProject | DesktopModal::EditProject(_) => {
                    self.project_modal(ui, modal)
                }
                DesktopModal::RemoveProject(project_id) => self.remove_modal(ui, project_id),
                DesktopModal::Discovery => self.discovery_modal(ui),
                DesktopModal::UpdateConfirmation => self.update_confirmation_modal(ui),
                DesktopModal::ApplyPatch => self.patch_confirmation_modal(ui, false),
                DesktopModal::RollbackPatch => self.patch_confirmation_modal(ui, true),
            });
        if response.should_close() {
            self.modal = None;
        }
    }

    fn project_modal(&mut self, ui: &mut egui::Ui, modal: DesktopModal) {
        let tokens = self.theme.tokens();
        ui.set_width(600.0);
        page_title(
            ui,
            if modal == DesktopModal::AddProject {
                "Add a project"
            } else {
                "Edit project details"
            },
            tokens,
        );
        muted_label(
            ui,
            "Choose what AppChecker monitors and how the project appears in your workspace.",
            tokens,
        );
        ui.add_space(12.0);
        egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
        section_heading(ui, "Repository folder", tokens);
        muted_label(
            ui,
            "The local Git repository whose manifests and dependency versions will be checked.",
            tokens,
        );
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.project_path)
                    .hint_text("/home/you/Projects/my-app")
                    .desired_width(390.0),
            );
            if secondary_button("Choose folder…", tokens).ui(ui).clicked() {
                let (sender, receiver) = std::sync::mpsc::channel();
                self.directory_receiver = Some(receiver);
                std::thread::spawn(move || {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        let _ = sender.send(path);
                    }
                });
            }
        });
        ui.add_space(16.0);
        section_heading(ui, "Display name", tokens);
        muted_label(
            ui,
            "Used in project lists and update confirmations. Leave empty to use the folder name.",
            tokens,
        );
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.project_name)
                .hint_text("Example: Customer portal")
                .desired_width(500.0),
        );
        ui.add_space(16.0);
        section_heading(ui, "Tags", tokens);
        muted_label(
            ui,
            "Optional labels for search and filtering. Separate several tags with commas.",
            tokens,
        );
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.project_tags)
                .hint_text("Example: backend, production, rust")
                .desired_width(500.0),
        );
        ui.add_space(16.0);
        ui.checkbox(
            &mut self.project_pinned,
            "Keep this project at the top of the list",
        );
        ui.checkbox(
            &mut self.project_trusted,
            "Allow package-manager lifecycle scripts for this trusted repository",
        );
        muted_label(
            ui,
            "Leave script trust disabled unless you understand and accept code executed by the package manager.",
            tokens,
        );
        if let Some(error) = &self.project_error {
            ui.add_space(8.0);
            ui.label(RichText::new(error).color(tokens.error));
        }
        });
        ui.add_space(16.0);
        ui.horizontal_wrapped(|ui| {
            let label = if modal == DesktopModal::AddProject {
                "Add project"
            } else {
                "Save changes"
            };
            if ui
                .add_enabled(
                    !self.project_path.trim().is_empty(),
                    primary_action(!self.project_path.trim().is_empty(), label, tokens),
                )
                .clicked()
            {
                let edit_id = match modal {
                    DesktopModal::EditProject(project_id) => Some(project_id),
                    _ => None,
                };
                if let Err(error) = self.save_project(edit_id, self.project_draft()) {
                    self.project_error = Some(error.to_string());
                }
            }
            if secondary_button("Cancel", tokens).ui(ui).clicked() {
                self.modal = None;
            }
        });
    }

    fn discovery_modal(&mut self, ui: &mut egui::Ui) {
        let tokens = self.theme.tokens();
        ui.set_width(560.0);
        page_title(ui, "Discovered repositories", tokens);
        ui.add_space(14.0);
        let paths = self.discovered_paths.clone().unwrap_or_default();
        if paths.is_empty() {
            muted_label(ui, "No new Git repositories found.", tokens);
        } else {
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    for path in &paths {
                        ui.label(RichText::new(path.display().to_string()).color(tokens.text));
                    }
                });
        }
        ui.add_space(18.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!paths.is_empty(), primary_button("Add all", tokens))
                .clicked()
                && let Err(error) = self.add_discovered()
            {
                self.activity.push(error.to_string());
                self.modal = None;
            }
            if secondary_button("Close", tokens).ui(ui).clicked() {
                self.modal = None;
            }
        });
    }

    fn remove_modal(&mut self, ui: &mut egui::Ui, project_id: ProjectId) {
        let tokens = self.theme.tokens();
        ui.set_width(460.0);
        page_title(ui, "Confirm removal", tokens);
        let project_name = self
            .config
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.name.clone())
            .unwrap_or_default();
        ui.add_space(12.0);
        ui.label(RichText::new(format!("Stop monitoring {project_name}?")).color(tokens.text));
        muted_label(
            ui,
            "Cached Git and dependency data will be removed. The repository itself is never deleted.",
            tokens,
        );
        ui.add_space(18.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if danger_button("Remove project", tokens).ui(ui).clicked()
                && let Err(error) = self.remove_project(project_id)
            {
                self.activity.push(error.to_string());
                self.modal = None;
            }
            if secondary_button("Cancel", tokens).ui(ui).clicked() {
                self.modal = None;
            }
        });
    }

    fn update_confirmation_modal(&mut self, ui: &mut egui::Ui) {
        let tokens = self.theme.tokens();
        ui.set_width(700.0);
        page_title(ui, self.pending_scope.title(), tokens);
        muted_label(
            ui,
            "Review each project, package and exact operation. The repository must still be clean at execution time.",
            tokens,
        );
        ui.add_space(12.0);
        egui::ScrollArea::vertical()
            .max_height(340.0)
            .show(ui, |ui| {
                let mut current_project = None;
                for pending in &self.pending_updates {
                    if current_project != Some(pending.project_id) {
                        current_project = Some(pending.project_id);
                        let name = self
                            .config
                            .projects
                            .iter()
                            .find(|project| project.id == pending.project_id)
                            .map(|project| project.name.clone())
                            .unwrap_or_else(|| "Unknown project".into());
                        section_heading(ui, &name, tokens);
                    }
                    let plan = &pending.plan;
                    ui.label(
                        RichText::new(format!(
                            "{} {} → {}",
                            plan.package, plan.current_version, plan.target_version
                        ))
                        .color(tokens.text),
                    );
                    muted_label(ui, &plan.command_line(), tokens);
                    ui.add_space(6.0);
                }
            });
        ui.add_space(16.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if primary_button(
                format!("Run {} update(s)", self.pending_updates.len()),
                tokens,
            )
            .ui(ui)
            .clicked()
            {
                self.start_pending_updates();
            }
            if secondary_button("Cancel", tokens).ui(ui).clicked() {
                self.pending_updates.clear();
                self.modal = None;
            }
        });
    }

    fn patch_confirmation_modal(&mut self, ui: &mut egui::Ui, rollback: bool) {
        let tokens = self.theme.tokens();
        ui.set_width(500.0);
        page_title(
            ui,
            if rollback {
                "Confirm rollback"
            } else {
                "Apply reviewed patch"
            },
            tokens,
        );
        ui.add_space(12.0);
        muted_label(
            ui,
            if rollback {
                "Rollback runs only if no files changed after the reviewed patch."
            } else {
                "The exact reviewed diff will be checked again, applied on a dedicated branch, then project checks will run."
            },
            tokens,
        );
        ui.add_space(18.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if rollback {
                "Roll back patch"
            } else {
                "Apply and run checks"
            };
            if primary_button(label, tokens).ui(ui).clicked() {
                if rollback {
                    self.rollback_ai_patch();
                } else {
                    self.apply_ai_patch();
                }
                self.modal = None;
            }
            if secondary_button("Cancel", tokens).ui(ui).clicked() {
                self.modal = None;
            }
        });
    }
}

pub fn run(config: AppConfig, config_path: PathBuf, store: Store) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AppChecker")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "appchecker",
        options,
        Box::new(move |_context| {
            let mut app = DesktopApp::new(config, config_path, store);
            app.refresh_all();
            app.refresh_ai_status();
            Ok(Box::new(app))
        }),
    )
}

use eframe::egui;

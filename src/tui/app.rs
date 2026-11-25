use super::events::{UserAction, parse_key};
use super::modals::Modal;
use super::state::{ConfigEvent, ConfigForm, FormEvent, ProjectForm, View};
use super::ui::{ButtonAction, UiRegions, ui};
use crate::config::{ConfigLoader, ProjectDraft, ProjectManager};
use crate::core::{
    AppConfig, DepInfo, GitState, Project, ProjectHealth, ProjectId, bounded_concurrency,
};
use crate::deps::{BatchCheck, CheckIssue, RegistryClient, UpdateManager, UpdatePlan};
use crate::git::GitScanner;
use crate::persist::Store;
use chrono::{DateTime, Local, Utc};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, mpsc};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct ActivityEntry {
    pub timestamp: DateTime<Local>,
    pub level: ActivityLevel,
    pub message: String,
}

#[derive(Debug)]
enum BackgroundEvent {
    ScanComplete {
        project_id: ProjectId,
        generation: u64,
        air_gap_mode: bool,
        git_result: Result<GitState, String>,
        check: BatchCheck,
        scanned_at: DateTime<Utc>,
    },
    DiscoveryComplete {
        paths: Vec<PathBuf>,
    },
    UpdateResult {
        project_id: ProjectId,
        package: String,
        result: Result<String, String>,
        finished: bool,
    },
}

pub struct TuiApp {
    pub(crate) config: AppConfig,
    pub(crate) health: HashMap<ProjectId, ProjectHealth>,
    pub(crate) dependencies: HashMap<ProjectId, Vec<DepInfo>>,
    pub(crate) registry_errors: HashMap<ProjectId, Vec<CheckIssue>>,
    pub(crate) manifest_errors: HashMap<ProjectId, Vec<crate::deps::ManifestError>>,
    pub(crate) non_public_projects: HashSet<ProjectId>,
    pub(crate) scanning: HashSet<ProjectId>,
    scan_generations: HashMap<ProjectId, u64>,
    scan_tasks: HashMap<ProjectId, tokio::task::JoinHandle<()>>,
    next_scan_generation: u64,
    pub(crate) updating: Option<ProjectId>,
    cancel_update: std::sync::Arc<std::sync::atomic::AtomicBool>,
    cancel_background: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) discovering: bool,
    pub(crate) selected_project: usize,
    pub(crate) selected_dependency: usize,
    pub(crate) view: View,
    pub(crate) modal: Option<Modal>,
    pub(crate) activity: Vec<ActivityEntry>,
    pub(crate) activity_scroll: usize,
    pub(crate) status: Option<(ActivityLevel, String)>,
    pub(crate) regions: UiRegions,
    config_path: PathBuf,
    store: Store,
    registry: std::sync::Arc<RegistryClient>,
    registry_semaphore: std::sync::Arc<Semaphore>,
    sender: mpsc::UnboundedSender<BackgroundEvent>,
    receiver: mpsc::UnboundedReceiver<BackgroundEvent>,
    last_auto_refresh: Instant,
    should_quit: bool,
}

impl TuiApp {
    pub fn new(config: AppConfig, config_path: PathBuf, store: Store) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let registry = std::sync::Arc::new(RegistryClient::new(
            config.registry_urls.clone(),
            config.air_gap_mode,
        ));
        let registry_semaphore = std::sync::Arc::new(Semaphore::new(bounded_concurrency(
            config.max_concurrent_scans,
        )));
        let mut app = Self {
            config,
            health: HashMap::new(),
            dependencies: HashMap::new(),
            registry_errors: HashMap::new(),
            manifest_errors: HashMap::new(),
            non_public_projects: HashSet::new(),
            scanning: HashSet::new(),
            scan_generations: HashMap::new(),
            scan_tasks: HashMap::new(),
            next_scan_generation: 0,
            updating: None,
            cancel_update: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cancel_background: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            discovering: false,
            selected_project: 0,
            selected_dependency: 0,
            view: View::Projects,
            modal: None,
            activity: Vec::new(),
            activity_scroll: 0,
            status: None,
            regions: UiRegions::default(),
            config_path,
            store,
            registry,
            registry_semaphore,
            sender,
            receiver,
            last_auto_refresh: Instant::now(),
            should_quit: false,
        };
        let _ = app.store.flush();
        app.reload_cached();
        app
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn modal(&self) -> Option<&Modal> {
        self.modal.as_ref()
    }

    pub fn set_regions(&mut self, regions: UiRegions) {
        self.regions = regions;
    }

    pub fn regions(&self) -> &UiRegions {
        &self.regions
    }

    pub fn activity(&self) -> &[ActivityEntry] {
        &self.activity
    }

    pub fn activity_scroll(&self) -> usize {
        self.activity_scroll
    }

    pub fn selected_dependency(&self) -> usize {
        self.selected_dependency
    }

    pub fn is_quitting(&self) -> bool {
        self.should_quit
    }

    pub fn is_scanning(&self) -> bool {
        !self.scanning.is_empty()
    }

    fn configured_scan_interval(&self) -> Duration {
        Duration::from_secs(self.config.scan_interval_secs.max(1))
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        let previous_hook =
            std::sync::Arc::new(std::sync::Mutex::new(Some(std::panic::take_hook())));
        let hook_state = previous_hook.clone();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = disable_raw_mode();
            let _ = execute!(
                io::stdout(),
                DisableMouseCapture,
                LeaveAlternateScreen,
                crossterm::cursor::Show
            );
            if let Some(hook) = hook_state.lock().unwrap().as_ref() {
                hook(panic_info);
            }
        }));
        if let Err(error) = enable_raw_mode() {
            std::panic::set_hook(previous_hook.lock().unwrap().take().unwrap());
            return Err(error.into());
        }
        let _cleanup = TerminalCleanup;
        let mut terminal = match execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)
            .and_then(|_| Terminal::new(CrosstermBackend::new(io::stdout())))
        {
            Ok(terminal) => terminal,
            Err(error) => {
                std::panic::set_hook(previous_hook.lock().unwrap().take().unwrap());
                return Err(error.into());
            }
        };
        info!("TUI started");
        self.refresh_all();

        let result = (|| -> anyhow::Result<()> {
            while !self.should_quit {
                self.process_background_events();
                if self.last_auto_refresh.elapsed() >= self.configured_scan_interval()
                    && self.scanning.is_empty()
                    && self.updating.is_none()
                    && !matches!(self.modal, Some(Modal::ConfirmUpdates { .. }))
                {
                    self.refresh_all();
                }

                let mut regions = UiRegions::default();
                terminal.draw(|frame| regions = ui(frame, self))?;
                self.regions = regions;
                if event::poll(Duration::from_millis(100))? {
                    match event::read()? {
                        Event::Key(key) if key.kind == KeyEventKind::Press => {
                            self.handle_key_event(key);
                        }
                        Event::Mouse(mouse) => self.handle_mouse(mouse),
                        _ => {}
                    }
                }
            }
            Ok(())
        })();
        let show_cursor_result = terminal.show_cursor();
        self.cancel_background
            .store(true, std::sync::atomic::Ordering::Relaxed);
        for (_, task) in self.scan_tasks.drain() {
            task.abort();
        }
        self.store.shutdown();
        std::panic::set_hook(previous_hook.lock().unwrap().take().unwrap());
        result?;
        show_cursor_result?;
        info!("TUI exited");
        Ok(())
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        if key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('c')
        {
            if self.updating.is_some() {
                self.cancel_update
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                self.status = Some((
                    ActivityLevel::Warning,
                    "Cancelling update; the terminal will close when the worker stops".into(),
                ));
            } else {
                self.should_quit = true;
            }
            return;
        }
        if self.modal.is_some() {
            self.handle_modal_key(key);
            return;
        }
        self.handle_action(parse_key(key));
    }

    pub fn handle_action(&mut self, action: UserAction) {
        match action {
            UserAction::MoveUp => self.move_selection(-1),
            UserAction::MoveDown => self.move_selection(1),
            UserAction::MoveToTop => self.select_first(),
            UserAction::MoveToBottom => self.select_last(),
            UserAction::SwitchView(index) => {
                self.view = View::from_index(index);
                self.selected_dependency = 0;
            }
            UserAction::ShowDependencies => self.view = View::Dependencies,
            UserAction::AddProject => {
                self.modal = Some(Modal::AddProject {
                    form: ProjectForm::new(),
                    error: None,
                });
            }
            UserAction::EditProject => {
                if let Some(project) = self.selected_project().cloned() {
                    self.modal = Some(Modal::EditProject {
                        project_id: project.id,
                        form: ProjectForm::from_project(&project),
                        error: None,
                    });
                }
            }
            UserAction::RemoveProject => {
                if let Some(project) = self.selected_project() {
                    self.modal = Some(Modal::RemoveProject {
                        project_id: project.id,
                        name: project.name.clone(),
                    });
                }
            }
            UserAction::DiscoverProjects => self.start_discovery(),
            UserAction::PinProject => self.toggle_pin(),
            UserAction::RefreshProject => self.refresh_selected(),
            UserAction::RefreshAll => self.refresh_all(),
            UserAction::UpdateDependency => self.prepare_updates(false),
            UserAction::UpdateAll => self.prepare_updates(true),
            UserAction::OpenConfig => {
                self.modal = Some(Modal::Config {
                    form: ConfigForm::new(
                        self.config.scan_interval_secs,
                        self.config.max_concurrent_scans,
                        self.config.air_gap_mode,
                    ),
                    error: None,
                });
            }
            UserAction::ShowHelp => self.modal = Some(Modal::Help),
            UserAction::Quit => {
                if self.updating.is_some() {
                    self.cancel_update
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    self.status = Some((
                        ActivityLevel::Warning,
                        "Cancelling update; the terminal will close when the worker stops".into(),
                    ));
                } else {
                    self.should_quit = true;
                }
            }
            UserAction::Close => {
                self.view = View::Projects;
                self.modal = None;
            }
            UserAction::None => {}
        }
    }

    fn handle_modal_key(&mut self, key: KeyEvent) {
        let Some(modal) = self.modal.as_mut() else {
            return;
        };
        match modal {
            Modal::AddProject { form, error } => match form.handle_key(key) {
                FormEvent::Submit(draft) => self.save_project(None, draft),
                FormEvent::Cancel => self.modal = None,
                FormEvent::Continue => {
                    *error = None;
                }
            },
            Modal::EditProject {
                project_id,
                form,
                error,
            } => {
                let id = *project_id;
                match form.handle_key(key) {
                    FormEvent::Submit(draft) => self.save_project(Some(id), draft),
                    FormEvent::Cancel => self.modal = None,
                    FormEvent::Continue => {
                        *error = None;
                    }
                }
            }
            Modal::RemoveProject { project_id, .. } => match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let id = *project_id;
                    self.remove_project(id);
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => self.modal = None,
                _ => {}
            },
            Modal::Discover { paths, selected } => match key.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Up | KeyCode::Char('k') => {
                    *selected = selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !paths.is_empty() {
                        *selected = (*selected + 1).min(paths.len() - 1);
                    }
                }
                KeyCode::Enter => {
                    if let Some(path) = paths.get(*selected).cloned() {
                        self.add_discovered(&[path]);
                    } else {
                        self.modal = None;
                    }
                }
                KeyCode::Char('a') => {
                    let paths = paths.clone();
                    self.add_discovered(&paths);
                }
                _ => {}
            },
            Modal::ConfirmUpdates {
                project_id,
                plans,
                selected,
            } => match key.code {
                KeyCode::Esc => self.modal = None,
                KeyCode::Up | KeyCode::Char('k') => {
                    *selected = selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !plans.is_empty() {
                        *selected = (*selected + 1).min(plans.len() - 1);
                    }
                }
                KeyCode::Enter => {
                    let id = *project_id;
                    let plans = plans.clone();
                    self.start_updates(id, plans);
                }
                _ => {}
            },
            Modal::Config { form, error } => match form.handle_key(key) {
                ConfigEvent::Submit {
                    scan_interval,
                    max_concurrent,
                    air_gap,
                } => self.save_config(scan_interval, max_concurrent, air_gap),
                ConfigEvent::Error(message) => {
                    *error = Some(message);
                }
                ConfigEvent::Cancel => self.modal = None,
                ConfigEvent::Continue => {}
            },
            Modal::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                    self.modal = None;
                }
            }
        }
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) {
        if matches!(mouse.kind, MouseEventKind::ScrollUp) {
            if self.modal.is_some() {
                self.scroll_modal(-1);
            } else {
                self.move_selection(-1);
            }
            return;
        }
        if matches!(mouse.kind, MouseEventKind::ScrollDown) {
            if self.modal.is_some() {
                self.scroll_modal(1);
            } else {
                self.move_selection(1);
            }
            return;
        }
        if !matches!(
            mouse.kind,
            MouseEventKind::Down(crossterm::event::MouseButton::Left)
        ) {
            return;
        }
        let x = mouse.column;
        let y = mouse.row;

        if self.modal.is_some() {
            if self
                .regions
                .modal_cancel
                .is_some_and(|area| contains(area, x, y))
            {
                self.modal = None;
                return;
            }
            if self
                .regions
                .modal_confirm
                .is_some_and(|area| contains(area, x, y))
            {
                self.confirm_modal(false);
                return;
            }
            if self
                .regions
                .modal_secondary
                .is_some_and(|area| contains(area, x, y))
            {
                self.confirm_modal(true);
                return;
            }
            if let Some((index, _)) = self
                .regions
                .modal_fields
                .iter()
                .find(|(_, area)| contains(*area, x, y))
            {
                let index = *index;
                match self.modal.as_mut() {
                    Some(Modal::AddProject { form, .. } | Modal::EditProject { form, .. }) => {
                        form.field = index;
                        if index == 3 {
                            form.pinned = !form.pinned;
                        }
                    }
                    Some(Modal::Config { form, .. }) => {
                        form.field = index;
                        if index == 2 {
                            form.air_gap = !form.air_gap;
                        }
                    }
                    _ => {}
                }
                return;
            }
            if let Some((index, _)) = self
                .regions
                .modal_rows
                .iter()
                .find(|(_, area)| contains(*area, x, y))
            {
                let index = *index;
                match self.modal.as_mut() {
                    Some(Modal::Discover { selected, .. })
                    | Some(Modal::ConfirmUpdates { selected, .. }) => *selected = index,
                    _ => {}
                }
            }
            return;
        }

        if let Some((view, _)) = self
            .regions
            .tabs
            .iter()
            .find(|(_, area)| contains(*area, x, y))
        {
            self.view = *view;
            return;
        }
        if let Some((project_id, _)) = self
            .regions
            .project_rows
            .iter()
            .find(|(_, area)| contains(*area, x, y))
        {
            if let Some(index) = self
                .config
                .projects
                .iter()
                .position(|project| project.id == *project_id)
            {
                self.selected_project = index;
                self.selected_dependency = 0;
            }
            return;
        }
        if let Some((index, _)) = self
            .regions
            .dependency_rows
            .iter()
            .find(|(_, area)| contains(*area, x, y))
        {
            self.selected_dependency = *index;
            return;
        }
        if let Some((action, _)) = self
            .regions
            .buttons
            .iter()
            .find(|(_, area)| contains(*area, x, y))
        {
            match action {
                ButtonAction::Add => self.handle_action(UserAction::AddProject),
                ButtonAction::Edit => self.handle_action(UserAction::EditProject),
                ButtonAction::Remove => self.handle_action(UserAction::RemoveProject),
                ButtonAction::Discover => self.handle_action(UserAction::DiscoverProjects),
                ButtonAction::Refresh => self.handle_action(UserAction::RefreshProject),
                ButtonAction::RefreshAll => self.handle_action(UserAction::RefreshAll),
                ButtonAction::Update => self.handle_action(UserAction::UpdateDependency),
                ButtonAction::UpdateAll => self.handle_action(UserAction::UpdateAll),
                ButtonAction::Config => self.handle_action(UserAction::OpenConfig),
                ButtonAction::Help => self.handle_action(UserAction::ShowHelp),
            }
        }
    }

    fn scroll_modal(&mut self, delta: isize) {
        match self.modal.as_mut() {
            Some(Modal::Discover { paths, selected }) => {
                if !paths.is_empty() {
                    *selected = selected.saturating_add_signed(delta).min(paths.len() - 1);
                }
            }
            Some(Modal::ConfirmUpdates {
                plans, selected, ..
            }) if !plans.is_empty() => {
                *selected = selected.saturating_add_signed(delta).min(plans.len() - 1);
            }
            _ => {}
        }
    }

    fn confirm_modal(&mut self, secondary: bool) {
        let Some(modal) = self.modal.as_ref() else {
            return;
        };
        match modal {
            Modal::AddProject { form, .. } => self.save_project(
                None,
                ProjectDraft {
                    path: form.path.clone().into(),
                    name: form.name.clone(),
                    tags: form
                        .tags
                        .split(',')
                        .map(|tag| tag.trim().to_string())
                        .filter(|tag| !tag.is_empty())
                        .collect(),
                    pinned: form.pinned,
                },
            ),
            Modal::EditProject {
                project_id, form, ..
            } => self.save_project(
                Some(*project_id),
                ProjectDraft {
                    path: form.path.clone().into(),
                    name: form.name.clone(),
                    tags: form
                        .tags
                        .split(',')
                        .map(|tag| tag.trim().to_string())
                        .filter(|tag| !tag.is_empty())
                        .collect(),
                    pinned: form.pinned,
                },
            ),
            Modal::RemoveProject { project_id, .. } => self.remove_project(*project_id),
            Modal::Discover { paths, selected } => {
                if paths.is_empty() {
                    self.modal = None;
                } else if secondary {
                    if let Some(path) = paths.get(*selected).cloned() {
                        self.add_discovered(&[path]);
                    }
                } else {
                    self.add_discovered(&paths.clone());
                }
            }
            Modal::ConfirmUpdates {
                project_id, plans, ..
            } => self.start_updates(*project_id, plans.clone()),
            Modal::Config { form, .. } => {
                let key = KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
                let mut form = form.clone();
                match form.handle_key(key) {
                    ConfigEvent::Submit {
                        scan_interval,
                        max_concurrent,
                        air_gap,
                    } => self.save_config(scan_interval, max_concurrent, air_gap),
                    ConfigEvent::Error(message) => {
                        if let Some(Modal::Config { error, .. }) = self.modal.as_mut() {
                            *error = Some(message);
                        }
                    }
                    _ => {}
                }
            }
            Modal::Help => self.modal = None,
        }
    }

    fn save_project(&mut self, project_id: Option<ProjectId>, draft: ProjectDraft) {
        if project_id.is_some_and(|id| self.updating == Some(id)) {
            self.status = Some((
                ActivityLevel::Warning,
                "Wait for the update to finish before editing this project".into(),
            ));
            return;
        }
        let previous = self.config.clone();
        let old_path = project_id.and_then(|id| {
            previous
                .projects
                .iter()
                .find(|project| project.id == id)
                .map(|project| project.path.clone())
        });
        let result = match project_id {
            Some(id) => ProjectManager::update(&mut self.config, id, draft),
            None => ProjectManager::add(&mut self.config, draft),
        };
        match result {
            Ok(project) => {
                if let Err(error) = ConfigLoader::save(&self.config, &self.config_path) {
                    self.config = previous;
                    self.set_modal_error(error.to_string());
                    return;
                }
                if old_path.as_ref() != Some(&project.path) {
                    if let Some(id) = project_id {
                        if let Some(task) = self.scan_tasks.remove(&id) {
                            task.abort();
                        }
                        self.next_scan_generation += 1;
                        self.scanning.remove(&id);
                        self.scan_generations.remove(&id);
                        self.health.remove(&id);
                        self.dependencies.remove(&id);
                        self.registry_errors.remove(&id);
                        self.manifest_errors.remove(&id);
                        self.non_public_projects.remove(&id);
                        if let Err(error) = self.store.clear_project_state(id) {
                            self.log(
                                ActivityLevel::Warning,
                                format!("Cannot clear stale project state: {error}"),
                            );
                        }
                    }
                }
                if let Err(error) = self.store.upsert_project(&project) {
                    self.log(
                        ActivityLevel::Warning,
                        format!("Project saved, cache sync failed: {error}"),
                    );
                }
                self.select_project(project.id);
                self.modal = None;
                self.log(
                    ActivityLevel::Success,
                    format!("Saved project {}", project.name),
                );
                self.refresh_project(project.id);
            }
            Err(error) => self.set_modal_error(error.to_string()),
        }
    }

    fn remove_project(&mut self, project_id: ProjectId) {
        if self.updating == Some(project_id) {
            self.status = Some((
                ActivityLevel::Warning,
                "Wait for the update to finish before removing this project".into(),
            ));
            return;
        }
        let previous = self.config.clone();
        match ProjectManager::remove(&mut self.config, project_id) {
            Ok(project) => {
                if let Err(error) = ConfigLoader::save(&self.config, &self.config_path) {
                    self.config = previous;
                    self.status = Some((ActivityLevel::Error, error.to_string()));
                    return;
                }
                if let Some(task) = self.scan_tasks.remove(&project_id) {
                    task.abort();
                }
                if let Err(error) = self.store.remove_project(project_id) {
                    self.log(
                        ActivityLevel::Warning,
                        format!("Cache removal failed: {error}"),
                    );
                }
                self.health.remove(&project_id);
                self.dependencies.remove(&project_id);
                self.registry_errors.remove(&project_id);
                self.manifest_errors.remove(&project_id);
                self.non_public_projects.remove(&project_id);
                self.scanning.remove(&project_id);
                self.scan_generations.remove(&project_id);
                self.selected_dependency = 0;
                self.selected_project = self
                    .selected_project
                    .min(self.config.projects.len().saturating_sub(1));
                self.modal = None;
                self.log(
                    ActivityLevel::Info,
                    format!("Removed project {}", project.name),
                );
            }
            Err(error) => self.status = Some((ActivityLevel::Error, error.to_string())),
        }
    }

    fn toggle_pin(&mut self) {
        let Some(project_id) = self.selected_project_id() else {
            return;
        };
        let previous = self.config.clone();
        match ProjectManager::toggle_pin(&mut self.config, project_id) {
            Ok(pinned) => {
                if let Err(error) = ConfigLoader::save(&self.config, &self.config_path) {
                    self.config = previous;
                    self.status = Some((ActivityLevel::Error, error.to_string()));
                    return;
                }
                if let Some(project) = self
                    .config
                    .projects
                    .iter()
                    .find(|project| project.id == project_id)
                    .cloned()
                {
                    if let Err(error) = self.store.upsert_project(&project) {
                        self.log(
                            ActivityLevel::Warning,
                            format!("Cannot synchronize {} in cache: {error}", project.name),
                        );
                    }
                    self.log(
                        ActivityLevel::Success,
                        format!(
                            "{} {}",
                            project.name,
                            if pinned { "pinned" } else { "unpinned" }
                        ),
                    );
                }
                self.select_project(project_id);
            }
            Err(error) => self.status = Some((ActivityLevel::Error, error.to_string())),
        }
    }

    fn start_discovery(&mut self) {
        if self.discovering {
            return;
        }
        self.discovering = true;
        self.status = Some((
            ActivityLevel::Info,
            "Discovering Git repositories...".into(),
        ));
        let paths = self.config.auto_discover_paths.clone();
        let depth = self.config.max_auto_discover_depth;
        let existing: Vec<PathBuf> = self
            .config
            .projects
            .iter()
            .map(|project| project.path.clone())
            .collect();
        let sender = self.sender.clone();
        let cancel = self.cancel_background.clone();
        tokio::task::spawn_blocking(move || {
            let paths = ConfigLoader::auto_discover_with_cancel(&paths, depth, &existing, cancel);
            let _ = sender.send(BackgroundEvent::DiscoveryComplete { paths });
        });
    }

    fn add_discovered(&mut self, paths: &[PathBuf]) {
        if paths.is_empty() {
            self.modal = None;
            return;
        }
        let selected_before = self.selected_project_id();
        let previous = self.config.clone();
        let mut added = Vec::new();
        for path in paths {
            if let Ok(project) = ProjectManager::add(
                &mut self.config,
                ProjectDraft {
                    path: path.clone(),
                    name: String::new(),
                    tags: Vec::new(),
                    pinned: false,
                },
            ) {
                added.push(project);
            }
        }
        if added.is_empty() {
            self.modal = None;
            return;
        }
        if let Err(error) = ConfigLoader::save(&self.config, &self.config_path) {
            self.config = previous;
            self.status = Some((ActivityLevel::Error, error.to_string()));
            return;
        }
        self.modal = None;
        for project in &added {
            if let Err(error) = self.store.upsert_project(project) {
                self.log(
                    ActivityLevel::Warning,
                    format!("Cannot synchronize {} in cache: {error}", project.name),
                );
            }
            self.refresh_project(project.id);
        }
        if let Some(project_id) = selected_before {
            self.select_project(project_id);
        } else if let Some(project) = added.first() {
            self.select_project(project.id);
        }
        self.log(
            ActivityLevel::Success,
            format!("Added {} discovered project(s)", added.len()),
        );
    }

    fn save_config(&mut self, scan_interval: u64, max_concurrent: usize, air_gap: bool) {
        let previous = self.config.clone();
        self.config.scan_interval_secs = scan_interval;
        self.config.max_concurrent_scans = bounded_concurrency(max_concurrent);
        self.config.air_gap_mode = air_gap;
        if let Err(save_error) = ConfigLoader::save(&self.config, &self.config_path) {
            self.config = previous;
            if let Some(Modal::Config { error, .. }) = self.modal.as_mut() {
                *error = Some(save_error.to_string());
            }
            return;
        }
        for (_, task) in self.scan_tasks.drain() {
            task.abort();
        }
        self.registry = std::sync::Arc::new(RegistryClient::new(
            self.config.registry_urls.clone(),
            self.config.air_gap_mode,
        ));
        self.registry_semaphore = std::sync::Arc::new(Semaphore::new(bounded_concurrency(
            self.config.max_concurrent_scans,
        )));
        self.next_scan_generation += 1;
        self.scanning.clear();
        self.scan_generations.clear();
        self.health.clear();
        self.dependencies.clear();
        self.registry_errors.clear();
        self.manifest_errors.clear();
        self.non_public_projects.clear();
        for project in self.config.projects.clone() {
            if let Err(error) = self.store.clear_project_state(project.id) {
                self.log(
                    ActivityLevel::Warning,
                    format!("Cannot clear stale project state: {error}"),
                );
            }
        }
        self.modal = None;
        self.log(ActivityLevel::Success, "Configuration saved".into());
        self.refresh_all();
    }

    fn set_modal_error(&mut self, message: String) {
        match self.modal.as_mut() {
            Some(
                Modal::AddProject { error, .. }
                | Modal::EditProject { error, .. }
                | Modal::Config { error, .. },
            ) => {
                *error = Some(message);
            }
            _ => self.status = Some((ActivityLevel::Error, message)),
        }
    }

    pub(crate) fn selected_project(&self) -> Option<&Project> {
        self.config.projects.get(self.selected_project)
    }

    pub(crate) fn selected_project_id(&self) -> Option<ProjectId> {
        self.selected_project().map(|project| project.id)
    }

    pub(crate) fn current_dependencies(&self) -> Vec<DepInfo> {
        self.selected_project_id()
            .and_then(|id| self.dependencies.get(&id).cloned())
            .unwrap_or_default()
    }

    pub(crate) fn current_health(&self) -> Option<&ProjectHealth> {
        self.selected_project_id()
            .and_then(|id| self.health.get(&id))
    }

    fn refresh_selected(&mut self) {
        if let Some(project_id) = self.selected_project_id() {
            self.refresh_project(project_id);
        }
    }

    fn refresh_all(&mut self) {
        self.last_auto_refresh = Instant::now();
        let project_ids: Vec<_> = self
            .config
            .projects
            .iter()
            .map(|project| project.id)
            .collect();
        for project_id in project_ids {
            self.refresh_project(project_id);
        }
    }

    fn refresh_project(&mut self, project_id: ProjectId) {
        if self.scanning.contains(&project_id) || self.updating == Some(project_id) {
            return;
        }
        let Some(project) = self
            .config
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
        else {
            return;
        };
        self.next_scan_generation += 1;
        let generation = self.next_scan_generation;
        self.scan_generations.insert(project_id, generation);
        self.scanning.insert(project_id);
        self.status = Some((ActivityLevel::Info, format!("Checking {}...", project.name)));
        let registry = self.registry.clone();
        let registry_semaphore = self.registry_semaphore.clone();
        let max_concurrent = self.config.max_concurrent_scans;
        let sender = self.sender.clone();
        let task = tokio::spawn(async move {
            let git_result = GitScanner::scan_repo_async(project.path.clone())
                .await
                .map_err(|error| error.to_string());
            let report = crate::deps::DepsParser::detect_and_report(&project.path);
            let mut check = registry
                .check_batch_detailed_with_limit(
                    &report.dependencies,
                    max_concurrent,
                    Some(registry_semaphore),
                )
                .await;
            check.manifest_errors = report.errors;
            check.non_public_skipped = report.non_public_skipped;
            let _ = sender.send(BackgroundEvent::ScanComplete {
                project_id: project.id,
                generation,
                air_gap_mode: registry.air_gap_mode(),
                git_result,
                check,
                scanned_at: Utc::now(),
            });
        });
        self.scan_tasks.insert(project_id, task);
    }

    pub fn process_background_events(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            self.handle_background(event);
        }
    }

    fn handle_background(&mut self, event: BackgroundEvent) {
        match event {
            BackgroundEvent::ScanComplete {
                project_id,
                generation,
                air_gap_mode,
                git_result,
                check,
                scanned_at,
            } => {
                if self.scan_generations.get(&project_id) != Some(&generation) {
                    return;
                }
                if !self
                    .config
                    .projects
                    .iter()
                    .any(|project| project.id == project_id)
                {
                    return;
                }
                self.scanning.remove(&project_id);
                self.scan_tasks.remove(&project_id);
                if let Ok(state) = &git_result {
                    if let Err(error) = self.store.save_git_state(project_id, state) {
                        self.log(
                            ActivityLevel::Warning,
                            format!("Cannot persist Git state: {error}"),
                        );
                    }
                }
                if let Err(error) = self.store.save_deps(project_id, &check.dependencies) {
                    self.log(
                        ActivityLevel::Warning,
                        format!("Cannot persist dependencies: {error}"),
                    );
                }
                let outdated = check
                    .dependencies
                    .iter()
                    .filter(|dependency| dependency.update_available)
                    .count() as u32;
                let security = check
                    .dependencies
                    .iter()
                    .filter(|dependency| {
                        dependency.update_available
                            && dependency.severity == crate::core::UpdateSeverity::Security
                    })
                    .count() as u32;
                let major = check
                    .dependencies
                    .iter()
                    .filter(|dependency| {
                        dependency.update_available
                            && dependency.severity == crate::core::UpdateSeverity::Major
                    })
                    .count() as u32;
                let scan_error = git_result.as_ref().err().map(|error| error.to_string());
                let git_state = git_result.ok();
                if let Some(error) = &scan_error {
                    self.log(ActivityLevel::Warning, format!("Git scan failed: {error}"));
                }
                for error in &check.manifest_errors {
                    self.log(ActivityLevel::Warning, error.to_string());
                }
                for issue in &check.errors {
                    self.log(
                        ActivityLevel::Warning,
                        format!(
                            "Registry {} ({}): {}",
                            issue.package, issue.ecosystem, issue.message
                        ),
                    );
                }
                self.health.insert(
                    project_id,
                    ProjectHealth {
                        project_id,
                        git_state,
                        outdated_deps_count: outdated,
                        security_updates_count: security,
                        major_updates_count: major,
                        last_scanned_at: Some(scanned_at),
                        scan_error: scan_error.clone(),
                    },
                );
                self.registry_errors
                    .insert(project_id, check.errors.clone());
                self.manifest_errors
                    .insert(project_id, check.manifest_errors.clone());
                if check.non_public_skipped {
                    self.non_public_projects.insert(project_id);
                } else {
                    self.non_public_projects.remove(&project_id);
                }
                self.dependencies
                    .insert(project_id, check.dependencies.clone());
                if Some(project_id) == self.selected_project_id() {
                    self.selected_dependency = self
                        .selected_dependency
                        .min(check.dependencies.len().saturating_sub(1));
                }
                let errors = check.errors.len();
                let manifest_error_count = check.manifest_errors.len();
                let level = if errors > 0 || manifest_error_count > 0 || scan_error.is_some() {
                    ActivityLevel::Warning
                } else {
                    ActivityLevel::Success
                };
                self.log(
                    level,
                    format!(
                        "Checked {}: {} dependencies, {} updates{}{}",
                        self.config
                            .projects
                            .iter()
                            .find(|project| project.id == project_id)
                            .map(|project| project.name.as_str())
                            .unwrap_or("project"),
                        check.dependencies.len(),
                        outdated,
                        if errors == 0 && manifest_error_count == 0 {
                            "".into()
                        } else {
                            format!(
                                ", {errors} registry errors, {manifest_error_count} manifest errors"
                            )
                        },
                        if air_gap_mode {
                            " (air-gap: registry checks skipped)"
                        } else {
                            ""
                        }
                    ),
                );
            }
            BackgroundEvent::DiscoveryComplete { paths } => {
                self.discovering = false;
                self.status = Some((
                    ActivityLevel::Info,
                    format!("{} new repositories found", paths.len()),
                ));
                if self.modal.is_none() {
                    self.modal = Some(Modal::Discover { paths, selected: 0 });
                }
            }
            BackgroundEvent::UpdateResult {
                project_id,
                package,
                result,
                finished,
            } => {
                match result {
                    Ok(output) => self.log(
                        ActivityLevel::Success,
                        format!("Updated {package}: {output}"),
                    ),
                    Err(error) => self.log(
                        ActivityLevel::Error,
                        format!("Update failed for {package}: {error}"),
                    ),
                }
                if finished {
                    self.updating = None;
                    if self
                        .cancel_update
                        .load(std::sync::atomic::Ordering::Relaxed)
                    {
                        self.should_quit = true;
                    } else {
                        self.refresh_project(project_id);
                    }
                }
            }
        }
    }

    fn prepare_updates(&mut self, all: bool) {
        if self.updating.is_some() {
            self.status = Some((
                ActivityLevel::Warning,
                "Another update is already running".into(),
            ));
            return;
        }
        let Some(project) = self.selected_project().cloned() else {
            self.status = Some((ActivityLevel::Warning, "Select a project first".into()));
            return;
        };
        if self.scanning.contains(&project.id) {
            self.status = Some((
                ActivityLevel::Warning,
                "Wait for the current check to finish before updating".into(),
            ));
            return;
        }
        let dependencies = self.current_dependencies();
        let targets: Vec<_> = if all {
            dependencies
                .into_iter()
                .filter(|dependency| dependency.update_available)
                .collect()
        } else {
            dependencies
                .get(
                    self.selected_dependency
                        .min(dependencies.len().saturating_sub(1)),
                )
                .filter(|dependency| dependency.update_available)
                .cloned()
                .into_iter()
                .collect()
        };
        if targets.is_empty() {
            self.status = Some((
                ActivityLevel::Info,
                "No update available for this selection".into(),
            ));
            return;
        }
        let mut plans = Vec::new();
        for dependency in targets {
            match UpdateManager::plan(&project.path, &dependency) {
                Ok(plan) => plans.push(plan),
                Err(error) => self.log(
                    ActivityLevel::Error,
                    format!("Cannot prepare {}: {error}", dependency.name),
                ),
            }
        }
        if plans.is_empty() {
            return;
        }
        self.modal = Some(Modal::ConfirmUpdates {
            project_id: project.id,
            plans,
            selected: 0,
        });
    }

    fn start_updates(&mut self, project_id: ProjectId, plans: Vec<UpdatePlan>) {
        if plans.is_empty() || self.updating.is_some() {
            return;
        }
        if self.scanning.contains(&project_id) {
            self.status = Some((
                ActivityLevel::Warning,
                "Wait for the current check to finish before updating".into(),
            ));
            return;
        }
        self.updating = Some(project_id);
        self.cancel_update
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.modal = None;
        self.log(
            ActivityLevel::Info,
            format!("Running {} update(s)", plans.len()),
        );
        let sender = self.sender.clone();
        let worker_sender = sender.clone();
        let cancel_update = self.cancel_update.clone();
        tokio::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let total = plans.len();
                for (index, plan) in plans.into_iter().enumerate() {
                    if cancel_update.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = worker_sender.send(BackgroundEvent::UpdateResult {
                            project_id,
                            package: "update cancelled".into(),
                            result: Err("Update cancelled by user".into()),
                            finished: true,
                        });
                        break;
                    }
                    let package = plan.package.clone();
                    let result = UpdateManager::execute_with_cancellation(
                        &plan,
                        Some(cancel_update.clone()),
                    )
                    .map(|outcome| {
                        if outcome.output.is_empty() {
                            format!("{} -> {}", plan.current_version, plan.target_version)
                        } else {
                            outcome.output
                        }
                    })
                    .map_err(|error| error.to_string());
                    let _ = worker_sender.send(BackgroundEvent::UpdateResult {
                        project_id,
                        package,
                        result,
                        finished: index + 1 == total,
                    });
                }
            })
            .await;
            if let Err(error) = result {
                let _ = sender.send(BackgroundEvent::UpdateResult {
                    project_id,
                    package: "update worker".into(),
                    result: Err(format!("Update worker failed: {error}")),
                    finished: true,
                });
            }
        });
    }

    fn reload_cached(&mut self) {
        for project in self.config.projects.clone() {
            if let Err(error) = self.store.upsert_project(&project) {
                self.log(
                    ActivityLevel::Warning,
                    format!("Cannot synchronize {} in cache: {error}", project.name),
                );
            }
            let git_state = self.store.get_git_state(project.id).ok().flatten();
            let dependencies = self.store.get_deps(project.id).unwrap_or_default();
            let outdated = dependencies
                .iter()
                .filter(|dependency| dependency.update_available)
                .count() as u32;
            let security = dependencies
                .iter()
                .filter(|dependency| {
                    dependency.update_available
                        && dependency.severity == crate::core::UpdateSeverity::Security
                })
                .count() as u32;
            let major = dependencies
                .iter()
                .filter(|dependency| {
                    dependency.update_available
                        && dependency.severity == crate::core::UpdateSeverity::Major
                })
                .count() as u32;
            let last_scanned_at = git_state
                .as_ref()
                .map(|state| state.scanned_at)
                .or_else(|| dependencies.first().map(|dependency| dependency.checked_at));
            self.health.insert(
                project.id,
                ProjectHealth {
                    project_id: project.id,
                    git_state,
                    outdated_deps_count: outdated,
                    security_updates_count: security,
                    major_updates_count: major,
                    last_scanned_at,
                    scan_error: None,
                },
            );
            self.dependencies.insert(project.id, dependencies);
        }
    }

    fn activity_scroll_limit(&self) -> usize {
        self.regions
            .activity_max_start
            .unwrap_or_else(|| self.activity.len().saturating_sub(1))
    }

    fn move_selection(&mut self, delta: isize) {
        match self.view {
            View::Projects => {
                let len = self.config.projects.len();
                if len > 0 {
                    self.selected_project = self
                        .selected_project
                        .saturating_add_signed(delta)
                        .min(len - 1);
                    self.selected_dependency = 0;
                }
            }
            View::Activity => {
                if delta > 0 {
                    self.activity_scroll = self.activity_scroll.saturating_sub(1);
                } else {
                    self.activity_scroll = self
                        .activity_scroll
                        .saturating_add(1)
                        .min(self.activity_scroll_limit());
                }
            }
            View::Dependencies => {
                let len = self.current_dependencies().len();
                if len > 0 {
                    self.selected_dependency = self
                        .selected_dependency
                        .saturating_add_signed(delta)
                        .min(len - 1);
                }
            }
        }
    }

    fn select_first(&mut self) {
        match self.view {
            View::Dependencies => self.selected_dependency = 0,
            View::Activity => self.activity_scroll = self.activity_scroll_limit(),
            View::Projects => {
                self.selected_project = 0;
                self.selected_dependency = 0;
            }
        }
    }

    fn select_last(&mut self) {
        match self.view {
            View::Dependencies => {
                self.selected_dependency = self.current_dependencies().len().saturating_sub(1);
            }
            View::Activity => self.activity_scroll = 0,
            View::Projects => {
                self.selected_project = self.config.projects.len().saturating_sub(1);
                self.selected_dependency = 0;
            }
        }
    }

    fn select_project(&mut self, project_id: ProjectId) {
        if let Some(index) = self
            .config
            .projects
            .iter()
            .position(|project| project.id == project_id)
        {
            self.selected_project = index;
            self.selected_dependency = 0;
        }
    }

    fn log(&mut self, level: ActivityLevel, message: String) {
        let lines = activity_lines(&message);
        self.status = Some((level, lines.last().cloned().unwrap_or_default()));
        for message in lines {
            self.activity.push(ActivityEntry {
                timestamp: Local::now(),
                level,
                message,
            });
            if self.activity.len() > 200 {
                self.activity.remove(0);
            }
        }
        self.activity_scroll = 0;
    }
}

struct TerminalCleanup;

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
    }
}

fn activity_lines(message: &str) -> Vec<String> {
    const MAX_LINES: usize = 40;
    const MAX_CHARACTERS: usize = 512;

    let mut lines: Vec<String> = message
        .lines()
        .map(|line| sanitize_terminal_text(line, MAX_CHARACTERS))
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.is_empty() {
        lines.push(String::new());
    }
    if lines.len() > MAX_LINES {
        let omitted = lines.len() - MAX_LINES;
        lines = lines.split_off(lines.len() - MAX_LINES);
        lines.insert(0, format!("… {omitted} earlier output lines omitted …"));
    }
    lines
}

fn sanitize_terminal_text(line: &str, max_characters: usize) -> String {
    let mut sanitized = String::new();
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            match characters.next() {
                Some('[') => {
                    for value in characters.by_ref() {
                        if ('@'..='~').contains(&value) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    let mut previous = '\0';
                    for value in characters.by_ref() {
                        if value == '\u{7}' || (previous == '\u{1b}' && value == '\\') {
                            break;
                        }
                        previous = value;
                    }
                }
                _ => {}
            }
        } else if character == '\t' {
            sanitized.push(' ');
        } else if !character.is_control() {
            sanitized.push(character);
        }
    }
    if sanitized.chars().count() > max_characters {
        let mut truncated: String = sanitized.chars().take(max_characters - 1).collect();
        truncated.push('…');
        truncated
    } else {
        sanitized
    }
}

fn contains(area: ratatui::layout::Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.x + area.width && y >= area.y && y < area.y + area.height
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn multiline_output_cannot_hide_new_activity_or_inject_ansi() {
        let root =
            std::env::temp_dir().join(format!("appchecker-activity-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&root.join("appchecker.db")).unwrap();
        let mut app = TuiApp::new(AppConfig::default(), root.join("config.toml"), store);
        let output = format!(
            "{}latest registry error",
            "\u{1b}[31mordinary output\n".repeat(60)
        );
        app.log(ActivityLevel::Error, output);
        app.view = View::Activity;

        assert!(app.activity.len() > 1);
        assert!(
            app.activity.iter().all(|entry| {
                !entry.message.contains('\n') && !entry.message.contains('\u{1b}')
            })
        );
        assert!(!app.status.as_ref().unwrap().1.contains('\u{1b}'));
        let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();
        terminal
            .draw(|frame| {
                ui(frame, &app);
            })
            .unwrap();
        let contents = format!("{:?}", terminal.backend().buffer());
        assert!(contents.contains("latest registry error"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn zero_scan_interval_is_clamped_to_one_second() {
        let root =
            std::env::temp_dir().join(format!("appchecker-interval-{}", uuid::Uuid::new_v4()));
        let store = Store::open(&root.join("appchecker.db")).unwrap();
        let app = TuiApp::new(
            AppConfig {
                scan_interval_secs: 0,
                ..AppConfig::default()
            },
            root.join("config.toml"),
            store,
        );
        assert_eq!(app.configured_scan_interval(), Duration::from_secs(1));
        std::fs::remove_dir_all(root).ok();
    }
}

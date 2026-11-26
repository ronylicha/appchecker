use super::DesktopApp;
use super::ai::{
    AiProvider, AnalysisResponse, AppliedPatch, ModelInfo, ProviderClient, ProviderStatus,
    ReviewedPatch, apply_reviewed_with_checks, launch_login, prepare_analysis_files,
};
use super::app::DesktopModal;
use super::theme::Tokens;
use super::widgets::{
    card_title, muted_label, page_title, pill_badge, primary_action, primary_button,
    secondary_button, section_heading,
};
use crate::core::ProjectId;
use eframe::egui;
use eframe::egui::{RichText, ScrollArea, Widget};
use std::path::PathBuf;

pub(crate) enum AiEvent {
    ProviderStatus {
        result: Result<ProviderStatus, String>,
        models: Result<Vec<ModelInfo>, String>,
    },
    Files {
        files: Vec<PathBuf>,
        rejected: usize,
    },
    Analysis {
        project_id: ProjectId,
        result: Result<AnalysisResponse, String>,
    },
    Applied {
        project_id: ProjectId,
        result: Result<(AppliedPatch, String), String>,
    },
    RolledBack(Result<String, String>),
}

impl DesktopApp {
    pub(crate) fn select_ai_provider(&mut self, provider: AiProvider) {
        self.ai_provider = provider;
        self.ai_status = None;
        self.ai_models.clear();
        self.ai_model = None;
        self.ai_analysis = None;
        self.ai_error = None;
        self.refresh_ai_status();
    }

    pub(crate) fn refresh_ai_status(&mut self) {
        if self.config.air_gap_mode {
            self.ai_status_busy = false;
            self.ai_error =
                Some("Provider status and model catalogs are unavailable in air-gap mode.".into());
            return;
        }
        self.ai_status_busy = true;
        let client = ProviderClient::new(self.ai_provider);
        let sender = self.ai_sender.clone();
        std::thread::spawn(move || {
            let result = client.status().map_err(|error| error.to_string());
            let models = client.models().map_err(|error| error.to_string());
            let _ = sender.send(AiEvent::ProviderStatus { result, models });
        });
    }

    pub(crate) fn connect_ai_provider(&mut self) {
        match launch_login(self.ai_provider) {
            Ok(()) => {
                self.ai_notice =
                    Some("Finish signing in in the terminal, then choose Refresh status.".into());
            }
            Err(error) => self.ai_error = Some(error.to_string()),
        }
    }

    pub(crate) fn choose_ai_files(&mut self) {
        let Some(project_id) = self.selected_project else {
            return;
        };
        let Some(project) = self
            .config
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .cloned()
        else {
            return;
        };
        let sender = self.ai_sender.clone();
        std::thread::spawn(move || {
            let mut files = Vec::new();
            let mut rejected = 0;
            for path in rfd::FileDialog::new()
                .set_directory(&project.path)
                .pick_files()
                .unwrap_or_default()
            {
                if let Ok(relative) = path.strip_prefix(&project.path) {
                    files.push(relative.to_path_buf());
                } else {
                    rejected += 1;
                }
            }
            let _ = sender.send(AiEvent::Files { files, rejected });
        });
    }

    pub(crate) fn start_ai_analysis(&mut self) {
        if self.ai_busy {
            return;
        }
        let Some(project_id) = self.selected_project else {
            self.ai_error = Some("Select a project first.".into());
            return;
        };
        if self.ai_provider == AiProvider::Antigravity && !self.antigravity_terms_accepted {
            self.ai_error = Some(
                "Review and accept the third-party Antigravity usage conditions before analysis."
                    .into(),
            );
            return;
        }
        if !self.ai_consent {
            self.ai_error = Some(
                "Confirm exactly which files may be sent before starting the analysis.".into(),
            );
            return;
        }
        if self.ai_files.is_empty() {
            self.ai_error = Some("Choose at least one source file to analyze.".into());
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
        self.ai_busy = true;
        self.ai_error = None;
        self.ai_notice = Some(format!(
            "{} is analyzing {} approved file(s); no commands are allowed.",
            self.ai_provider.label(),
            self.ai_files.len()
        ));
        let client = ProviderClient::new(self.ai_provider).with_model(self.ai_model.clone());
        let files = self.ai_files.clone();
        let root = self.backup_root.join("ai-analysis");
        let sender = self.ai_sender.clone();
        let air_gap = self.config.air_gap_mode;
        std::thread::spawn(move || {
            let result = prepare_analysis_files(&project.path, &files, &root)
                .map_err(|error| error.to_string())
                .and_then(|prepared| {
                    client
                        .analyze(&prepared, air_gap)
                        .map_err(|error| error.to_string())
                });
            let _ = sender.send(AiEvent::Analysis { project_id, result });
        });
    }

    pub(crate) fn apply_ai_patch(&mut self) {
        if self.ai_busy {
            return;
        }
        let Some(project_id) = self.selected_project else {
            return;
        };
        let Some(analysis) = self.ai_analysis.clone() else {
            self.ai_error = Some("Run an analysis before applying a patch.".into());
            return;
        };
        if analysis.patch.trim().is_empty() {
            self.ai_error = Some("The provider proposed no patch to apply.".into());
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
        self.ai_busy = true;
        self.ai_error = None;
        let sender = self.ai_sender.clone();
        let files = self.ai_files.clone();
        let backup_root = self.backup_root.join("ai-patches");
        std::thread::spawn(move || {
            let result =
                ReviewedPatch::review(&project.path, &analysis.patch, &files, &backup_root)
                    .map_err(|error| error.to_string())
                    .and_then(|reviewed| {
                        apply_reviewed_with_checks(reviewed).map_err(|error| error.to_string())
                    });
            let _ = sender.send(AiEvent::Applied { project_id, result });
        });
    }

    pub(crate) fn rollback_ai_patch(&mut self) {
        if self.ai_busy {
            return;
        }
        let Some(applied) = self.ai_applied.take() else {
            return;
        };
        self.ai_busy = true;
        let sender = self.ai_sender.clone();
        std::thread::spawn(move || {
            let result = applied
                .rollback()
                .map(|_| "Patch rolled back without overwriting later edits.".into())
                .map_err(|error| error.to_string());
            let _ = sender.send(AiEvent::RolledBack(result));
        });
    }

    pub(crate) fn process_ai_events(&mut self) {
        while let Ok(event) = self.ai_receiver.try_recv() {
            match event {
                AiEvent::ProviderStatus { result, models } => {
                    self.ai_status_busy = false;
                    match result {
                        Ok(status) => self.ai_status = Some(status),
                        Err(error) => self.ai_error = Some(error),
                    }
                    match models {
                        Ok(models) => self.ai_models = models,
                        Err(error) => self.ai_error = Some(error),
                    }
                }
                AiEvent::Files { files, rejected } => {
                    self.ai_files = files;
                    self.ai_consent = false;
                    self.ai_analysis = None;
                    if rejected > 0 {
                        self.ai_notice = Some(format!(
                            "{rejected} file(s) outside the project were rejected."
                        ));
                    }
                }
                AiEvent::Analysis { project_id, result } => {
                    self.ai_busy = false;
                    if self.selected_project != Some(project_id) {
                        continue;
                    }
                    match result {
                        Ok(analysis) => {
                            self.ai_analysis = Some(analysis);
                            self.ai_notice = Some(
                                "Analysis received. Review every finding and the complete patch."
                                    .into(),
                            );
                        }
                        Err(error) => self.ai_error = Some(error),
                    }
                }
                AiEvent::Applied { project_id, result } => {
                    self.ai_busy = false;
                    if self.selected_project != Some(project_id) {
                        continue;
                    }
                    match result {
                        Ok((applied, checks)) => {
                            self.ai_applied = Some(applied);
                            self.ai_analysis = None;
                            self.ai_notice = Some(checks);
                            self.refresh_project(project_id);
                        }
                        Err(error) => self.ai_error = Some(error),
                    }
                }
                AiEvent::RolledBack(result) => {
                    self.ai_busy = false;
                    match result {
                        Ok(message) => {
                            self.ai_notice = Some(message);
                            if let Some(project_id) = self.selected_project {
                                self.refresh_project(project_id);
                            }
                        }
                        Err(error) => {
                            self.ai_error = Some(error);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn remediation_view(&mut self, ui: &mut egui::Ui) {
        ScrollArea::vertical().show(ui, |ui| self.remediation_content(ui));
    }

    fn remediation_content(&mut self, ui: &mut egui::Ui) {
        let tokens = self.theme.tokens();
        page_title(ui, "AI remediation", tokens);
        muted_label(
            ui,
            "Analyze breaking changes with an official provider. Nothing changes without your review.",
            tokens,
        );
        ui.add_space(16.0);
        self.provider_panel(ui, tokens);
        ui.add_space(16.0);
        self.project_switcher(ui);
        ui.add_space(12.0);
        let Some(project_id) = self.selected_project else {
            muted_label(ui, "Select a project in Projects to begin.", tokens);
            return;
        };
        let project_name = self
            .config
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.name.clone())
            .unwrap_or_default();
        section_heading(ui, &project_name, tokens);
        ui.horizontal(|ui| {
            if secondary_button("Choose source files…", tokens)
                .ui(ui)
                .clicked()
            {
                self.choose_ai_files();
            }
            if self.ai_busy {
                ui.spinner();
                muted_label(ui, "Working…", tokens);
            }
        });
        ui.add_space(8.0);
        if self.ai_files.is_empty() {
            muted_label(
                ui,
                "No files selected. Manifests and source files are never sent implicitly.",
                tokens,
            );
        } else {
            let files = self.ai_files.clone();
            for (index, file) in files.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    pill_badge(ui, &file.display().to_string(), tokens.text, tokens);
                    if ui
                        .add(egui::Button::new(
                            RichText::new(format!("Remove {}", file.display())).color(tokens.error),
                        ))
                        .clicked()
                    {
                        self.ai_files.remove(index);
                        self.ai_consent = false;
                        self.ai_analysis = None;
                    }
                });
            }
            let consent = format!(
                "I authorize sending only these {} file(s), after redaction, to {}.",
                self.ai_files.len(),
                self.ai_provider.label()
            );
            ui.checkbox(&mut self.ai_consent, consent);
            if ui
                .add_enabled(
                    self.ai_consent
                        && !self.ai_busy
                        && (self.ai_provider != AiProvider::Antigravity
                            || self.antigravity_terms_accepted),
                    primary_action(
                        self.ai_consent
                            && !self.ai_busy
                            && (self.ai_provider != AiProvider::Antigravity
                                || self.antigravity_terms_accepted),
                        "Analyze breaking changes",
                        tokens,
                    ),
                )
                .clicked()
            {
                self.start_ai_analysis();
            }
        }
        self.ai_messages(ui, tokens);
        ui.add_space(12.0);
        if let Some(analysis) = self.ai_analysis.clone() {
            self.analysis_panel(ui, tokens, analysis);
        }
        if self.ai_applied.is_some()
            && ui
                .add_enabled(
                    !self.ai_busy,
                    secondary_button("Roll back reviewed patch", tokens),
                )
                .clicked()
        {
            self.modal = Some(DesktopModal::RollbackPatch);
        }
    }

    fn provider_panel(&mut self, ui: &mut egui::Ui, tokens: Tokens) {
        ui.horizontal_wrapped(|ui| {
            for provider in [
                AiProvider::Claude,
                AiProvider::Codex,
                AiProvider::Antigravity,
            ] {
                let selected = self.ai_provider == provider;
                if secondary_button(provider.label(), tokens)
                    .fill(if selected {
                        tokens.selection
                    } else {
                        tokens.surface
                    })
                    .ui(ui)
                    .clicked()
                {
                    self.select_ai_provider(provider);
                }
            }
            if ui
                .add_enabled(
                    !self.ai_status_busy,
                    secondary_button("Refresh status", tokens),
                )
                .clicked()
            {
                self.refresh_ai_status();
            }
            if primary_button("Connect with OAuth", tokens)
                .ui(ui)
                .clicked()
            {
                self.connect_ai_provider();
            }
            if self.ai_status_busy {
                ui.spinner();
            }
        });
        ui.add_space(8.0);
        if self.ai_models.is_empty() {
            if !self.ai_status_busy {
                muted_label(
                    ui,
                    "No model catalog loaded. Refresh the provider status.",
                    tokens,
                );
            }
        } else {
            let selected = self
                .ai_model
                .as_ref()
                .and_then(|id| {
                    self.ai_models
                        .iter()
                        .find(|model| &model.id == id)
                        .map(|model| model.label.clone())
                })
                .unwrap_or_else(|| "Provider default".into());
            let models = self.ai_models.clone();
            egui::ComboBox::from_label("Model")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.ai_model.is_none(), "Provider default")
                        .clicked()
                    {
                        self.ai_model = None;
                        self.ai_analysis = None;
                    }
                    for model in models {
                        if ui
                            .selectable_label(
                                self.ai_model.as_deref() == Some(model.id.as_str()),
                                &model.label,
                            )
                            .clicked()
                        {
                            self.ai_model = Some(model.id);
                            self.ai_analysis = None;
                        }
                    }
                });
        }
        if self.ai_provider == AiProvider::Antigravity {
            ui.add_space(8.0);
            muted_label(
                ui,
                "Google does not support third-party headless use on consumer plans. Use a supported Enterprise/API route or accept responsibility for this run.",
                tokens,
            );
            ui.checkbox(
                &mut self.antigravity_terms_accepted,
                "I understand and accept the conditions for this Antigravity run.",
            );
        }
        match &self.ai_status {
            Some(ProviderStatus::Authenticated(detail)) => {
                pill_badge(ui, "Connected", tokens.success, tokens);
                muted_label(ui, detail, tokens);
            }
            Some(ProviderStatus::Unauthenticated) => {
                pill_badge(ui, "Not connected", tokens.warning, tokens);
            }
            None => {
                muted_label(ui, "Provider status has not been checked yet.", tokens);
            }
        }
    }

    fn ai_messages(&mut self, ui: &mut egui::Ui, tokens: Tokens) {
        if let Some(error) = &self.ai_error {
            ui.label(RichText::new(error).color(tokens.error));
        }
        if let Some(notice) = &self.ai_notice {
            muted_label(ui, notice, tokens);
        }
    }

    fn analysis_panel(&mut self, ui: &mut egui::Ui, tokens: Tokens, analysis: AnalysisResponse) {
        ui.separator();
        section_heading(ui, "Review findings and patch", tokens);
        for finding in &analysis.findings {
            card_title(
                ui,
                &format!("{}: {}", finding.package, finding.summary),
                tokens,
            );
            for change in &finding.breaking_changes {
                muted_label(ui, &format!("• {change}"), tokens);
            }
        }
        if analysis.patch.is_empty() {
            muted_label(ui, "No patch was proposed.", tokens);
            return;
        }
        ScrollArea::both().max_height(300.0).show(ui, |ui| {
            for line in analysis.patch.lines() {
                let color = if line.starts_with('+') {
                    tokens.success
                } else if line.starts_with('-') {
                    tokens.error
                } else if line.starts_with("@@") {
                    tokens.accent_text
                } else {
                    tokens.text
                };
                ui.label(RichText::new(line).monospace().color(color));
            }
        });
        if ui
            .add_enabled(
                !self.ai_busy,
                primary_action(!self.ai_busy, "Apply reviewed patch and run checks", tokens),
            )
            .clicked()
        {
            self.modal = Some(DesktopModal::ApplyPatch);
        }
    }
}

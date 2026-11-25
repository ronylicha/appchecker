use super::app::{ActivityLevel, TuiApp};
use super::modals::draw_modal;
use super::state::View;
use super::theme;
use crate::core::DepInfo;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonAction {
    Add,
    Edit,
    Remove,
    Discover,
    Refresh,
    RefreshAll,
    Update,
    UpdateAll,
    Config,
    Help,
}

#[derive(Debug, Default)]
pub struct UiRegions {
    pub tabs: Vec<(View, Rect)>,
    pub project_rows: Vec<(crate::core::ProjectId, Rect)>,
    pub dependency_rows: Vec<(usize, Rect)>,
    pub buttons: Vec<(ButtonAction, Rect)>,
    pub modal_confirm: Option<Rect>,
    pub modal_secondary: Option<Rect>,
    pub modal_cancel: Option<Rect>,
    pub modal_fields: Vec<(usize, Rect)>,
    pub modal_rows: Vec<(usize, Rect)>,
    pub activity_max_start: Option<usize>,
}

pub fn draw(frame: &mut Frame, app: &TuiApp) -> UiRegions {
    ui(frame, app)
}

pub fn ui(frame: &mut Frame, app: &TuiApp) -> UiRegions {
    let mut regions = UiRegions::default();
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(frame.area());
    draw_header(frame, app, root[0]);
    draw_tabs(frame, app, root[1], &mut regions);
    if frame.area().width < 80 {
        let body = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(8), Constraint::Min(8)])
            .split(root[2]);
        draw_projects(frame, app, body[0], &mut regions);
        draw_content(frame, app, body[1], &mut regions);
    } else {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(32), Constraint::Min(45)])
            .split(root[2]);
        draw_projects(frame, app, body[0], &mut regions);
        draw_content(frame, app, body[1], &mut regions);
    }
    draw_footer(frame, app, root[3]);
    if let Some(modal) = &app.modal {
        draw_modal(frame, modal, &mut regions);
    }
    regions
}

fn draw_header(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let total: usize = app.dependencies.values().map(Vec::len).sum();
    let updates: usize = app
        .dependencies
        .values()
        .flatten()
        .filter(|dependency| dependency.update_available)
        .count();
    let has_errors = app
        .health
        .values()
        .any(|health| health.scan_error.is_some())
        || app
            .registry_errors
            .values()
            .any(|errors| !errors.is_empty())
        || app
            .manifest_errors
            .values()
            .any(|errors| !errors.is_empty());
    let has_unchecked = app
        .dependencies
        .values()
        .flatten()
        .any(|dependency| dependency.latest_version.is_none());
    let has_ranges = app
        .dependencies
        .values()
        .flatten()
        .any(dependency_has_unresolved_range);
    let has_non_public = !app.non_public_projects.is_empty();
    let update_style = if has_errors || updates > 0 {
        Style::default().fg(theme::WARNING)
    } else if app.config.air_gap_mode || has_unchecked || has_ranges || has_non_public {
        theme::muted()
    } else {
        Style::default().fg(theme::SUCCESS)
    };
    let operation = if app.updating.is_some() {
        "Updating dependencies..."
    } else if app.discovering {
        "Discovering repositories..."
    } else if app.scanning.is_empty() {
        "Ready"
    } else {
        "Checking projects..."
    };
    let air_gap = if app.config.air_gap_mode {
        "  |  AIR-GAP"
    } else {
        ""
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(" APPCHECKER ", theme::action()),
            Span::styled("Local project health", theme::muted()),
            Span::raw(" ".repeat(area.width.saturating_sub(45) as usize)),
            Span::styled(operation, theme::action()),
        ]),
        Line::from(vec![
            Span::styled(
                format!(" {} projects", app.config.projects.len()),
                theme::title(),
            ),
            Span::styled(format!("  |  {total} dependencies"), theme::muted()),
            Span::styled(
                format!("  |  {updates} updates available{air_gap}"),
                update_style,
            ),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(theme::muted()),
        ),
        area,
    );
}

fn draw_tabs(frame: &mut Frame, app: &TuiApp, area: Rect, regions: &mut UiRegions) {
    let tabs = [
        (View::Projects, "1 Projects"),
        (View::Dependencies, "2 Dependencies"),
        (View::Activity, "3 Activity"),
    ];
    let widths = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(18),
            Constraint::Length(22),
            Constraint::Length(18),
            Constraint::Min(1),
        ])
        .split(area);
    for (index, (view, label)) in tabs.into_iter().enumerate() {
        let selected = app.view == view;
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!(" {label} "),
                if selected {
                    theme::action()
                } else {
                    theme::muted()
                },
            )),
            widths[index],
        );
        regions.tabs.push((view, widths[index]));
    }
}

fn draw_projects(frame: &mut Frame, app: &TuiApp, area: Rect, regions: &mut UiRegions) {
    let block = Block::default()
        .title(" Projects ")
        .borders(Borders::ALL)
        .border_style(theme::muted());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if app.config.projects.is_empty() {
        draw_empty(
            frame,
            inner,
            "No projects yet",
            "Press a or click Add to start",
        );
        return;
    }
    let visible = (inner.height / 2).max(1) as usize;
    let start = app
        .selected_project
        .saturating_sub(visible.saturating_sub(1));
    for (row, (index, project)) in app
        .config
        .projects
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .enumerate()
    {
        let health = app.health.get(&project.id);
        let outdated = health.map(|health| health.outdated_deps_count).unwrap_or(0);
        let scanning = app.scanning.contains(&project.id);
        let branch = health
            .and_then(|health| health.git_state.as_ref())
            .and_then(|state| state.current_branch.as_deref())
            .unwrap_or("not scanned");
        let git = health
            .and_then(|health| health.git_state.as_ref())
            .map(|state| {
                let mut value = String::new();
                if state.is_dirty {
                    value.push('*');
                }
                if state.ahead > 0 {
                    value.push_str(&format!(" +{}", state.ahead));
                }
                if state.behind > 0 {
                    value.push_str(&format!(" -{}", state.behind));
                }
                if value.is_empty() {
                    " clean".into()
                } else {
                    value
                }
            })
            .unwrap_or_else(|| " ?".into());
        let selected = index == app.selected_project;
        let y = inner.y + (row * 2) as u16;
        let row_area = Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: 2,
        };
        let pin = if project.pinned { "[p] " } else { "    " };
        let registry_errors = app
            .registry_errors
            .get(&project.id)
            .map(Vec::len)
            .unwrap_or(0)
            + app
                .manifest_errors
                .get(&project.id)
                .map(Vec::len)
                .unwrap_or(0);
        let unchecked_dependencies = app
            .dependencies
            .get(&project.id)
            .map(|dependencies| {
                dependencies
                    .iter()
                    .any(|dependency| dependency.latest_version.is_none())
            })
            .unwrap_or(false);
        let range_dependencies = app
            .dependencies
            .get(&project.id)
            .map(|dependencies| dependencies.iter().any(dependency_has_unresolved_range))
            .unwrap_or(false);
        let status = if scanning {
            "checking".to_string()
        } else if health.is_none() || health.and_then(|health| health.last_scanned_at).is_none() {
            "not checked".to_string()
        } else if registry_errors > 0
            || health
                .and_then(|health| health.scan_error.as_ref())
                .is_some()
        {
            "check errors".to_string()
        } else if app.config.air_gap_mode {
            "air-gap".to_string()
        } else if unchecked_dependencies {
            "not checked".to_string()
        } else if outdated > 0 {
            format!("{outdated} updates")
        } else if app.non_public_projects.contains(&project.id) {
            "private/local".to_string()
        } else if range_dependencies {
            "range limits".to_string()
        } else {
            "up to date".to_string()
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(pin, theme::muted()),
                Span::styled(project.name.clone(), row_style(selected)),
                Span::styled(format!("  {status}"), row_style(selected)),
            ])),
            row_area,
        );
        let detail_area = Rect {
            y: y + 1,
            ..row_area
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("    {branch}{git}"),
                if selected {
                    theme::selected()
                } else {
                    theme::muted()
                },
            ))),
            detail_area,
        );
        regions.project_rows.push((project.id, row_area));
    }
}

fn draw_content(frame: &mut Frame, app: &TuiApp, area: Rect, regions: &mut UiRegions) {
    match app.view {
        View::Projects => draw_overview(frame, app, area, regions),
        View::Dependencies => draw_dependencies(frame, app, area, regions),
        View::Activity => draw_activity(frame, app, area, regions),
    }
}

fn draw_overview(frame: &mut Frame, app: &TuiApp, area: Rect, regions: &mut UiRegions) {
    let Some(project) = app.selected_project() else {
        let block = Block::default()
            .title(" Overview ")
            .borders(Borders::ALL)
            .border_style(theme::muted());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        draw_empty(
            frame,
            inner,
            "Your workspace is empty",
            "Add a Git repository or discover projects automatically",
        );
        draw_actions(
            frame,
            app,
            Rect {
                y: area.y + area.height.saturating_sub(2),
                height: 2,
                ..area
            },
            regions,
            true,
        );
        return;
    };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Length(3),
            Constraint::Min(6),
        ])
        .split(area);
    let health = app.current_health();
    let summary = Block::default()
        .title(format!(" {} ", project.name))
        .borders(Borders::ALL)
        .border_style(theme::muted());
    let summary_inner = summary.inner(sections[0]);
    frame.render_widget(summary, sections[0]);
    let git = health.and_then(|health| health.git_state.as_ref());
    let branch = git
        .and_then(|state| state.current_branch.as_deref())
        .unwrap_or("unknown");
    let git_status = git
        .map(|state| {
            format!(
                "{} dirty  |  {} staged  |  {} untracked  |  +{} ahead  |  -{} behind  |  {} stashes",
                if state.is_dirty { "Has" } else { "No" },
                state.staged_count,
                state.untracked_count,
                state.ahead,
                state.behind,
                state.stash_count
            )
        })
        .unwrap_or_else(|| "Git has not been scanned yet".into());
    let scan = health
        .and_then(|health| health.last_scanned_at)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "Never".into());
    let mut lines = vec![
        Line::from(Span::styled(
            ellipsize_start(
                &project.path.display().to_string(),
                summary_inner.width as usize,
            ),
            theme::muted(),
        )),
        Line::from(vec![
            Span::styled("Branch  ", theme::muted()),
            Span::styled(branch, theme::title()),
            Span::raw("  |  "),
            Span::styled(git_status, theme::title()),
        ]),
        Line::from(vec![
            Span::styled("Last check  ", theme::muted()),
            Span::styled(scan, theme::title()),
        ]),
    ];
    if let Some(error) = health.and_then(|health| health.scan_error.as_deref()) {
        lines.push(Line::from(Span::styled(
            error.to_string(),
            Style::default().fg(theme::ERROR),
        )));
    }
    frame.render_widget(Paragraph::new(lines), summary_inner);
    draw_actions(frame, app, sections[1], regions, true);
    draw_dependency_table(
        frame,
        app,
        sections[2],
        regions,
        "Dependencies preview",
        true,
    );
}

fn draw_dependencies(frame: &mut Frame, app: &TuiApp, area: Rect, regions: &mut UiRegions) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8)])
        .split(area);
    draw_actions(frame, app, sections[0], regions, false);
    draw_dependency_table(
        frame,
        app,
        sections[1],
        regions,
        "Dependency verification",
        false,
    );
}

fn draw_dependency_table(
    frame: &mut Frame,
    app: &TuiApp,
    area: Rect,
    regions: &mut UiRegions,
    title: &str,
    preview: bool,
) {
    let dependencies = app.current_dependencies();
    let selected_dependency = app
        .selected_dependency
        .min(dependencies.len().saturating_sub(1));
    let errors = app
        .selected_project_id()
        .and_then(|id| app.registry_errors.get(&id))
        .map(Vec::len)
        .unwrap_or(0)
        + app
            .selected_project_id()
            .and_then(|id| app.manifest_errors.get(&id))
            .map(Vec::len)
            .unwrap_or(0);
    let outdated = dependencies
        .iter()
        .filter(|dependency| dependency.update_available)
        .count();
    let title = format!(
        " {title}  |  {} total  |  {outdated} updates  |  {errors} errors ",
        dependencies.len()
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(theme::muted());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if app.config.projects.is_empty() {
        draw_empty(
            frame,
            inner,
            "No project selected",
            "Add a project to inspect dependencies",
        );
        return;
    }
    if dependencies.is_empty() {
        let (title, detail) = if app
            .selected_project_id()
            .is_some_and(|id| app.scanning.contains(&id))
        {
            (
                "Checking dependency manifests",
                "Registry results will appear here",
            )
        } else if app
            .selected_project_id()
            .and_then(|id| app.manifest_errors.get(&id))
            .map(Vec::len)
            .unwrap_or(0)
            > 0
        {
            (
                "Manifests could not be parsed",
                "Review manifest errors in Activity",
            )
        } else if errors > 0 {
            (
                "Dependencies could not be checked",
                "Review registry errors in Activity",
            )
        } else if app
            .selected_project_id()
            .is_some_and(|id| app.non_public_projects.contains(&id))
        {
            (
                "Private/local dependencies skipped",
                "No public registry checks were performed",
            )
        } else if app.config.air_gap_mode {
            (
                "Local manifests are empty",
                "Air-gap mode skips registry version checks",
            )
        } else {
            (
                "No supported dependency manifest found",
                "npm, Cargo, Go, pip, Composer and Gem are supported",
            )
        };
        draw_empty(frame, inner, title, detail);
        return;
    }
    let visible = inner.height.saturating_sub(2).max(1) as usize;
    let start = selected_dependency.saturating_sub(visible.saturating_sub(1));
    let table = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!("{:<28}", "PACKAGE"), theme::muted()),
            Span::styled(format!("{:<10}", "ECOSYSTEM"), theme::muted()),
            Span::styled(format!("{:<16}", "CURRENT"), theme::muted()),
            Span::styled(format!("{:<16}", "LATEST"), theme::muted()),
            Span::styled("STATUS", theme::muted()),
        ])),
        table[0],
    );
    for (row, (index, dependency)) in dependencies
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .enumerate()
    {
        let selected = index == selected_dependency;
        let row_area = Rect {
            x: table[1].x,
            y: table[1].y + row as u16,
            width: table[1].width,
            height: 1,
        };
        let has_error = app
            .selected_project_id()
            .and_then(|id| app.registry_errors.get(&id))
            .map(|errors| {
                errors.iter().any(|error| {
                    error.package == dependency.name
                        && error.ecosystem == dependency.ecosystem
                        && error.dev_dependency == dependency.dev_dependency
                        && error.manifest_name == dependency.manifest_name
                })
            })
            .unwrap_or(false);
        let status = if app.config.air_gap_mode {
            ("AIR-GAP", theme::muted())
        } else {
            dependency_status(dependency, has_error)
        };
        let line = dependency_line(dependency, status, selected, table[1].width);
        frame.render_widget(Paragraph::new(line), row_area);
        regions.dependency_rows.push((index, row_area));
    }
    if !preview {
        if let Some(dependency) = dependencies.get(selected_dependency) {
            if dependency.update_available {
                if let Ok(plan) = crate::deps::UpdateManager::plan(
                    &app.selected_project().unwrap().path,
                    dependency,
                ) {
                    let command_area = Rect {
                        x: inner.x,
                        y: inner.y + inner.height.saturating_sub(1),
                        width: inner.width,
                        height: 1,
                    };
                    frame.render_widget(
                        Paragraph::new(Line::from(vec![
                            Span::styled("Remedy  ", theme::muted()),
                            Span::styled(plan.command_line(), theme::action()),
                        ])),
                        command_area,
                    );
                }
            }
        }
    }
}

fn draw_actions(
    frame: &mut Frame,
    app: &TuiApp,
    area: Rect,
    regions: &mut UiRegions,
    project_actions: bool,
) {
    let mut actions: Vec<(ButtonAction, &str)> = if project_actions {
        vec![
            (ButtonAction::Add, "+ Add"),
            (ButtonAction::Edit, "Edit"),
            (ButtonAction::Remove, "Remove"),
            (ButtonAction::Discover, "Discover"),
            (ButtonAction::Refresh, "Refresh"),
            (ButtonAction::RefreshAll, "Refresh all"),
            (ButtonAction::UpdateAll, "Update all"),
            (ButtonAction::Config, "Config"),
            (ButtonAction::Help, "Help"),
        ]
    } else {
        vec![
            (ButtonAction::Refresh, "Refresh"),
            (ButtonAction::RefreshAll, "Refresh all"),
            (ButtonAction::Update, "Update selected"),
            (ButtonAction::UpdateAll, "Update all"),
            (ButtonAction::Help, "Help"),
        ]
    };
    if app.config.projects.is_empty() {
        actions.retain(|(action, _)| {
            matches!(
                action,
                ButtonAction::Add | ButtonAction::Discover | ButtonAction::Help
            )
        });
    }
    let mut x = area.x;
    let mut y = area.y;
    for (action, label) in actions {
        let width = label.len() as u16 + 4;
        if x + width > area.x + area.width {
            x = area.x;
            y += 1;
        }
        if y >= area.y + area.height || x + width > area.x + area.width {
            break;
        }
        let button_area = Rect {
            x,
            y,
            width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(Span::styled(format!("[ {label} ]"), theme::action())),
            button_area,
        );
        regions.buttons.push((action, button_area));
        x += width + 1;
    }
    if app.updating.is_some() && y == area.y {
        frame.render_widget(
            Paragraph::new(Span::styled("Update in progress...", theme::muted())),
            Rect {
                x: area.x,
                y: area.y + 1,
                width: area.width,
                height: 1,
            },
        );
    }
}

fn draw_activity(frame: &mut Frame, app: &TuiApp, area: Rect, regions: &mut UiRegions) {
    let block = Block::default()
        .title(format!(" Activity ({}) ", app.activity.len()))
        .borders(Borders::ALL)
        .border_style(theme::muted());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if app.activity.is_empty() {
        draw_empty(
            frame,
            inner,
            "No activity yet",
            "Scans and updates will be recorded here",
        );
        return;
    }
    let visible = inner.height as usize;
    let max_start = app.activity.len().saturating_sub(visible);
    regions.activity_max_start = Some(max_start);
    let start = max_start.saturating_sub(app.activity_scroll.min(max_start));
    let lines: Vec<Line> = app
        .activity
        .iter()
        .skip(start)
        .take(visible)
        .map(|entry| {
            Line::from(vec![
                Span::styled(
                    entry.timestamp.format("%H:%M:%S").to_string(),
                    theme::muted(),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("{:<8}", level_label(entry.level)),
                    Style::default().fg(level_color(entry.level)),
                ),
                Span::styled(entry.message.clone(), theme::title()),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_footer(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let help = if app.modal.is_some() {
        "Enter: confirm  |  Esc: cancel  |  Mouse: click fields and buttons"
    } else {
        "a:add e:edit d:remove f:discover r:refresh u:update U:update all c:config h:help q:quit"
    };
    let lines = vec![Line::from(Span::styled(help, theme::muted()))];
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(theme::muted()),
        ),
        area,
    );
    if let Some((level, message)) = &app.status {
        let status_area = Rect {
            y: area.y + 1,
            height: 1,
            ..area
        };
        frame.render_widget(Clear, status_area);
        frame.render_widget(
            Paragraph::new(Span::styled(
                message.as_str(),
                Style::default().fg(level_color(*level)),
            )),
            status_area,
        );
    }
}

fn draw_empty(frame: &mut Frame, area: Rect, title: &str, detail: &str) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(title, theme::title())),
            Line::from(Span::styled(detail, theme::muted())),
        ])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: false }),
        area,
    );
}

fn dependency_line(
    dependency: &DepInfo,
    status: (&str, Style),
    selected: bool,
    width: u16,
) -> Line<'static> {
    let (status, status_style) = status;
    let latest = dependency
        .latest_version
        .as_deref()
        .unwrap_or("not checked");
    let dev = if dependency.dev_dependency {
        " (dev)"
    } else {
        ""
    };
    let row = format!(
        "{:<28}{:<10}{:<16}{:<16}",
        format!("{}{}", dependency.name, dev),
        dependency.ecosystem.to_string(),
        dependency.current_version,
        latest
    );
    let available_width = width.saturating_sub(14) as usize;
    let row = if row.chars().count() > available_width {
        format!(
            "{}...",
            row.chars()
                .take(available_width.saturating_sub(3))
                .collect::<String>()
        )
    } else {
        row
    };
    let style = if selected {
        theme::selected()
    } else {
        Style::default().fg(theme::TEXT)
    };
    Line::from(vec![
        Span::styled(format!("{row:<available_width$}"), style),
        Span::styled(
            status.to_string(),
            if selected { style } else { status_style },
        ),
    ])
}

fn ellipsize_start(text: &str, width: usize) -> String {
    if width == 0 || text.chars().count() <= width {
        return text.to_string();
    }
    if width == 1 {
        return "…".into();
    }
    let tail: String = text
        .chars()
        .rev()
        .take(width - 1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

fn dependency_has_unresolved_range(dependency: &DepInfo) -> bool {
    !crate::deps::RegistryClient::is_comparable(&dependency.current_version)
        || dependency
            .latest_version
            .as_deref()
            .is_some_and(|version| !crate::deps::RegistryClient::is_comparable(version))
}

fn dependency_status(dependency: &DepInfo, has_error: bool) -> (&'static str, Style) {
    if has_error {
        return ("ERROR", Style::default().fg(theme::ERROR));
    }
    if dependency.update_available {
        match dependency.severity {
            crate::core::UpdateSeverity::Major => ("MAJOR", Style::default().fg(theme::WARNING)),
            crate::core::UpdateSeverity::Security => {
                ("SECURITY", Style::default().fg(theme::ERROR))
            }
            _ => ("UPDATE", Style::default().fg(theme::WARNING)),
        }
    } else if dependency.latest_version.is_some()
        && (!crate::deps::RegistryClient::is_comparable(&dependency.current_version)
            || dependency
                .latest_version
                .as_deref()
                .map(|version| !crate::deps::RegistryClient::is_comparable(version))
                .unwrap_or(true))
    {
        ("RANGE", theme::muted())
    } else if dependency.latest_version.is_some() {
        ("CURRENT", Style::default().fg(theme::SUCCESS))
    } else {
        ("NOT CHECKED", theme::muted())
    }
}

fn row_style(selected: bool) -> Style {
    if selected {
        theme::selected()
    } else {
        Style::default().fg(theme::TEXT)
    }
}

fn level_label(level: ActivityLevel) -> &'static str {
    match level {
        ActivityLevel::Info => "INFO",
        ActivityLevel::Success => "OK",
        ActivityLevel::Warning => "WARNING",
        ActivityLevel::Error => "ERROR",
    }
}

fn level_color(level: ActivityLevel) -> ratatui::style::Color {
    match level {
        ActivityLevel::Info => theme::ACCENT,
        ActivityLevel::Success => theme::SUCCESS,
        ActivityLevel::Warning => theme::WARNING,
        ActivityLevel::Error => theme::ERROR,
    }
}

use crate::core::{AppConfig, ProjectHealth, UpdateSeverity};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

/// Draw the complete TUI dashboard.
pub fn draw(frame: &mut Frame, health: &[ProjectHealth], config: &AppConfig, selected: usize) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(5),    // Project list
            Constraint::Length(3), // Status bar
        ])
        .split(frame.area());

    // ── Header ──────────────────────────────────────────────
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " appchecker ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" │ {} projects monitored", config.projects.len())),
        Span::raw(" │ "),
        Span::styled("q", Style::default().fg(Color::Yellow)),
        Span::raw(":quit "),
        Span::styled("j/k", Style::default().fg(Color::Yellow)),
        Span::raw(":nav "),
        Span::styled("p", Style::default().fg(Color::Yellow)),
        Span::raw(":pin "),
        Span::styled("r", Style::default().fg(Color::Yellow)),
        Span::raw(":rescan"),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(header, chunks[0]);

    // ── Project List ────────────────────────────────────────
    let items: Vec<ListItem> = health
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let project = config.projects.iter().find(|p| p.id == h.project_id);
            let name = project.map(|p| p.name.as_str()).unwrap_or("unknown");
            let pinned = project.map(|p| p.pinned).unwrap_or(false);

            // Build status indicators
            let git_indicator = match &h.git_state {
                Some(gs) => {
                    let mut parts = Vec::new();
                    if gs.is_dirty {
                        parts.push(Span::styled("*", Style::default().fg(Color::Red)));
                    }
                    if gs.ahead > 0 {
                        parts.push(Span::styled(
                            format!("↑{}", gs.ahead),
                            Style::default().fg(Color::Green),
                        ));
                    }
                    if gs.behind > 0 {
                        parts.push(Span::styled(
                            format!("↓{}", gs.behind),
                            Style::default().fg(Color::Yellow),
                        ));
                    }
                    if parts.is_empty() {
                        vec![Span::styled("✓", Style::default().fg(Color::Green))]
                    } else {
                        parts
                    }
                }
                None => vec![Span::styled("?", Style::default().fg(Color::DarkGray))],
            };

            let dep_indicator = if h.security_updates_count > 0 {
                Span::styled(
                    format!(" 🔴{}sec", h.security_updates_count),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )
            } else if h.major_updates_count > 0 {
                Span::styled(
                    format!(" 🟡{}maj", h.major_updates_count),
                    Style::default().fg(Color::Yellow),
                )
            } else if h.outdated_deps_count > 0 {
                Span::styled(
                    format!(" 🔵{}out", h.outdated_deps_count),
                    Style::default().fg(Color::Blue),
                )
            } else {
                Span::styled(" ✓deps", Style::default().fg(Color::Green))
            };

            let pin_marker = if pinned {
                Span::styled("📌 ", Style::default().fg(Color::Magenta))
            } else {
                Span::raw("   ")
            };

            let error_marker = if let Some(err) = &h.scan_error {
                Span::styled(format!(" ⚠ {}", err), Style::default().fg(Color::Red))
            } else {
                Span::raw("")
            };

            let mut spans = vec![pin_marker];
            spans.extend(git_indicator);
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                name,
                Style::default().add_modifier(Modifier::BOLD),
            ));
            spans.push(dep_indicator);
            spans.push(error_marker);

            let style = if i == selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Projects ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray));

    frame.render_widget(list, chunks[1]);

    // ── Status Bar ──────────────────────────────────────────
    let air_gap_label = if config.air_gap_mode {
        Span::styled(
            " [AIR-GAP] ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw("")
    };

    let status = Paragraph::new(Line::from(vec![
        Span::raw(" Scan interval: "),
        Span::styled(
            format!("{}s", config.scan_interval_secs),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(" │ Max concurrent: "),
        Span::styled(
            format!("{}", config.max_concurrent_scans),
            Style::default().fg(Color::Cyan),
        ),
        air_gap_label,
        Span::raw(" │ "),
        Span::styled(
            format!("Selected: {}/{}", selected + 1, health.len()),
            Style::default().fg(Color::DarkGray),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(status, chunks[2]);
}

use super::state::{ConfigForm, ProjectForm};
use super::theme;
use super::ui::UiRegions;
use crate::core::ProjectId;
use crate::deps::UpdatePlan;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Modal {
    AddProject {
        form: ProjectForm,
        error: Option<String>,
    },
    EditProject {
        project_id: ProjectId,
        form: ProjectForm,
        error: Option<String>,
    },
    RemoveProject {
        project_id: ProjectId,
        name: String,
    },
    Discover {
        paths: Vec<PathBuf>,
        selected: usize,
    },
    ConfirmUpdates {
        project_id: ProjectId,
        plans: Vec<UpdatePlan>,
        selected: usize,
    },
    Config {
        form: ConfigForm,
        error: Option<String>,
    },
    Help,
}

pub fn draw_modal(frame: &mut Frame, modal: &Modal, regions: &mut UiRegions) {
    let area = centered_rect(82, 82, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(format!(" {} ", title(modal)))
        .borders(Borders::ALL)
        .border_style(theme::action());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    match modal {
        Modal::AddProject { form, error } | Modal::EditProject { form, error, .. } => {
            draw_project_form(frame, form, error.as_deref(), inner, regions);
        }
        Modal::RemoveProject { name, .. } => {
            draw_lines(
                frame,
                inner,
                vec![
                    Line::from("Stop monitoring this project?"),
                    Line::from(""),
                    Line::from(Span::styled(name.clone(), theme::title())),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Cached Git and dependency data will also be removed.",
                        theme::muted(),
                    )),
                ],
            );
            buttons(frame, inner, "Remove", None, regions);
        }
        Modal::Discover { paths, selected } => {
            let list_area = Rect {
                height: inner.height.saturating_sub(4),
                ..inner
            };
            if paths.is_empty() {
                draw_lines(
                    frame,
                    list_area,
                    vec![Line::from("No new Git repositories found.")],
                );
                buttons(frame, inner, "Close", None, regions);
            } else {
                let visible = list_area.height.max(1) as usize;
                let start = selected.saturating_sub(visible.saturating_sub(1));
                let lines: Vec<Line> = paths
                    .iter()
                    .enumerate()
                    .skip(start)
                    .take(visible)
                    .map(|(index, path)| {
                        Line::from(Span::styled(
                            format!(
                                "{} {}",
                                marker(index == *selected),
                                ellipsize_start(
                                    &path.display().to_string(),
                                    list_area.width.saturating_sub(2) as usize,
                                )
                            ),
                            row_style(index == *selected),
                        ))
                    })
                    .collect();
                draw_lines_truncated(frame, list_area, lines);
                regions.modal_rows = paths
                    .iter()
                    .enumerate()
                    .skip(start)
                    .take(visible)
                    .map(|(index, _)| {
                        (
                            index,
                            Rect {
                                x: list_area.x,
                                y: list_area.y + (index - start) as u16,
                                width: list_area.width,
                                height: 1,
                            },
                        )
                    })
                    .collect();
                buttons(frame, inner, "Add all", Some("Add selected"), regions);
            }
        }
        Modal::ConfirmUpdates {
            plans, selected, ..
        } => {
            let list_area = Rect {
                height: inner.height.saturating_sub(6),
                ..inner
            };
            let visible = (list_area.height.saturating_sub(1) / 2).max(1) as usize;
            let start = selected.saturating_sub(visible.saturating_sub(1));
            let mut lines = vec![Line::from(Span::styled(
                "Review every operation before running them:",
                theme::muted(),
            ))];
            for (index, plan) in plans.iter().enumerate().skip(start).take(visible) {
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} {} {} -> {}",
                        marker(index == *selected),
                        plan.package,
                        plan.current_version,
                        plan.target_version
                    ),
                    row_style(index == *selected),
                )));
                lines.push(Line::from(Span::styled(
                    format!("  {}", plan.command_line()),
                    if index == *selected {
                        theme::action()
                    } else {
                        theme::muted()
                    },
                )));
            }
            draw_lines_truncated(frame, list_area, lines);
            regions.modal_rows = plans
                .iter()
                .enumerate()
                .skip(start)
                .take(visible)
                .map(|(index, _)| {
                    let row = index - start;
                    (
                        index,
                        Rect {
                            x: list_area.x,
                            y: list_area.y + 1 + (row * 2) as u16,
                            width: list_area.width,
                            height: 2,
                        },
                    )
                })
                .collect();
            if let Some(plan) = plans.get(*selected) {
                let command_area = Rect {
                    y: inner.y + inner.height.saturating_sub(4),
                    height: 2,
                    ..inner
                };
                draw_lines(
                    frame,
                    command_area,
                    vec![Line::from(Span::styled(
                        plan.command_line(),
                        theme::action(),
                    ))],
                );
            }
            let confirm = format!("Run {} update(s)", plans.len());
            buttons(frame, inner, &confirm, None, regions);
        }
        Modal::Config { form, error } => {
            draw_config_form(frame, form, error.as_deref(), inner, regions);
        }
        Modal::Help => {
            draw_lines(
                frame,
                inner,
                vec![
                    Line::from(Span::styled("Navigation", theme::title())),
                    Line::from("j/k or arrows: select    g/G: first/last"),
                    Line::from("1/2/3: views             Enter: dependencies"),
                    Line::from("Mouse: click rows, tabs and buttons; wheel: scroll"),
                    Line::from(""),
                    Line::from(Span::styled("Projects", theme::title())),
                    Line::from("a: add    e: edit    d: remove    f: discover    p: pin"),
                    Line::from(""),
                    Line::from(Span::styled("Verification and updates", theme::title())),
                    Line::from("r: refresh selected      R: refresh all"),
                    Line::from("u: update selected       U: review all updates"),
                    Line::from(""),
                    Line::from("c: configuration    h/?: help    q: quit    Esc: close"),
                ],
            );
            buttons(frame, inner, "Close", None, regions);
        }
    }
}

fn draw_project_form(
    frame: &mut Frame,
    form: &ProjectForm,
    error: Option<&str>,
    area: Rect,
    regions: &mut UiRegions,
) {
    let values = [&form.path, &form.name, &form.tags];
    let labels = ["Git path", "Name", "Tags (comma separated)"];
    for (index, (label, value)) in labels.into_iter().zip(values).enumerate() {
        let y = area.y + index as u16 * 2;
        draw_lines_truncated(
            frame,
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
            vec![Line::from(vec![
                Span::styled(format!("{label:<24}"), label_style(form.field == index)),
                Span::styled(
                    ellipsize_start(
                        &format!("{value}{}", cursor(form.field == index)),
                        area.width.saturating_sub(24) as usize,
                    ),
                    theme::title(),
                ),
            ])],
        );
        regions.modal_fields.push((
            index,
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
        ));
    }
    let pin_y = area.y + 6;
    draw_lines(
        frame,
        Rect {
            x: area.x,
            y: pin_y,
            width: area.width,
            height: 1,
        },
        vec![Line::from(vec![
            Span::styled(format!("{:<24}", "Pinned"), label_style(form.field == 3)),
            Span::styled(
                if form.pinned { "[x] yes" } else { "[ ] no" },
                theme::title(),
            ),
        ])],
    );
    regions.modal_fields.push((
        3,
        Rect {
            x: area.x,
            y: pin_y,
            width: area.width,
            height: 1,
        },
    ));
    draw_lines(
        frame,
        Rect {
            x: area.x,
            y: area.y + 8,
            width: area.width,
            height: 1,
        },
        vec![Line::from(Span::styled(
            "Tab: next field    Space: toggle pin    Enter: save",
            theme::muted(),
        ))],
    );
    if let Some(error) = error {
        draw_lines(
            frame,
            Rect {
                x: area.x,
                y: area.y + 9,
                width: area.width,
                height: 2,
            },
            vec![Line::from(Span::styled(
                error.to_string(),
                Style::default().fg(theme::ERROR),
            ))],
        );
    }
    buttons(frame, area, "Save project", None, regions);
}

fn draw_config_form(
    frame: &mut Frame,
    form: &ConfigForm,
    error: Option<&str>,
    area: Rect,
    regions: &mut UiRegions,
) {
    let rows = [
        ("Scan interval (seconds)", form.scan_interval.as_str()),
        ("Concurrent registry checks", form.max_concurrent.as_str()),
    ];
    for (index, (label, value)) in rows.into_iter().enumerate() {
        let y = area.y + index as u16 * 2;
        draw_lines(
            frame,
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
            vec![Line::from(vec![
                Span::styled(format!("{label:<30}"), label_style(form.field == index)),
                Span::styled(
                    format!("{value}{}", cursor(form.field == index)),
                    theme::title(),
                ),
            ])],
        );
        regions.modal_fields.push((
            index,
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
        ));
    }
    let air_gap_y = area.y + 4;
    draw_lines(
        frame,
        Rect {
            x: area.x,
            y: air_gap_y,
            width: area.width,
            height: 1,
        },
        vec![Line::from(vec![
            Span::styled(
                format!("{:<30}", "Air-gap (no registry calls)"),
                label_style(form.field == 2),
            ),
            Span::styled(
                if form.air_gap {
                    "[x] enabled"
                } else {
                    "[ ] disabled"
                },
                theme::title(),
            ),
        ])],
    );
    regions.modal_fields.push((
        2,
        Rect {
            x: area.x,
            y: air_gap_y,
            width: area.width,
            height: 1,
        },
    ));
    if let Some(error) = error {
        draw_lines(
            frame,
            Rect {
                x: area.x,
                y: area.y + 7,
                width: area.width,
                height: 2,
            },
            vec![Line::from(Span::styled(
                error.to_string(),
                Style::default().fg(theme::ERROR),
            ))],
        );
    }
    buttons(frame, area, "Save configuration", None, regions);
}

fn buttons(
    frame: &mut Frame,
    area: Rect,
    confirm: &str,
    secondary: Option<&str>,
    regions: &mut UiRegions,
) {
    let y = area.y + area.height.saturating_sub(2);
    let confirm_area = Rect {
        x: area.x,
        y,
        width: confirm.len() as u16 + 4,
        height: 1,
    };
    let secondary_area = secondary.map(|label| Rect {
        x: confirm_area.x + confirm_area.width + 2,
        y,
        width: label.len() as u16 + 4,
        height: 1,
    });
    let cancel_label = "Cancel";
    let cancel_x = secondary_area
        .map(|secondary| secondary.x + secondary.width + 2)
        .unwrap_or(confirm_area.x + confirm_area.width + 2);
    let cancel_area = Rect {
        x: cancel_x,
        y,
        width: cancel_label.len() as u16 + 4,
        height: 1,
    };
    let mut spans = vec![Span::styled(format!("[ {confirm} ]"), theme::action())];
    if let Some(label) = secondary {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(format!("[ {label} ]"), theme::action()));
    }
    spans.push(Span::raw("  "));
    spans.push(Span::styled("[ Cancel ]", theme::muted()));
    draw_lines(
        frame,
        Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        },
        vec![Line::from(spans)],
    );
    regions.modal_confirm = Some(confirm_area);
    regions.modal_secondary = secondary_area;
    regions.modal_cancel = Some(cancel_area);
}

fn draw_lines(frame: &mut Frame, area: Rect, lines: Vec<Line>) {
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_lines_truncated(frame: &mut Frame, area: Rect, lines: Vec<Line>) {
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Left), area);
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

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn title(modal: &Modal) -> String {
    match modal {
        Modal::AddProject { .. } => "Add project".into(),
        Modal::EditProject { .. } => "Edit project".into(),
        Modal::RemoveProject { .. } => "Confirm removal".into(),
        Modal::Discover { paths, .. } => format!("Discovered repositories ({})", paths.len()),
        Modal::ConfirmUpdates { .. } => "Confirm dependency updates".into(),
        Modal::Config { .. } => "Configuration".into(),
        Modal::Help => "Keyboard and mouse help".into(),
    }
}

fn marker(selected: bool) -> &'static str {
    if selected { ">" } else { " " }
}

fn row_style(selected: bool) -> Style {
    if selected {
        theme::selected()
    } else {
        Style::default().fg(theme::TEXT)
    }
}

fn label_style(selected: bool) -> Style {
    if selected {
        theme::action()
    } else {
        theme::muted()
    }
}

fn cursor(selected: bool) -> &'static str {
    if selected { "_" } else { "" }
}

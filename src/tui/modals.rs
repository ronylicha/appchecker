use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::path::PathBuf;

/// Modal dialog types for visual project management
#[derive(Debug, Clone)]
pub enum Modal {
    AddProject {
        path_input: String,
        name_input: String,
        tags_input: String,
        pin: bool,
        cursor_field: usize, // 0=path, 1=name, 2=tags
    },
    RemoveConfirm {
        project_name: String,
    },
    DiscoverResults {
        paths: Vec<PathBuf>,
        selected: usize,
    },
    ConfigEditor {
        scan_interval: String,
        max_concurrent: String,
        air_gap: bool,
        cursor_field: usize,
    },
    Help,
    ProjectDetail {
        name: String,
        branch: String,
        dirty: bool,
        ahead: u32,
        behind: u32,
        deps_total: u32,
        deps_outdated: u32,
        deps_security: u32,
    },
}

impl Modal {
    pub fn new_add_project() -> Self {
        Modal::AddProject {
            path_input: String::new(),
            name_input: String::new(),
            tags_input: String::new(),
            pin: false,
            cursor_field: 0,
        }
    }

    pub fn new_remove_confirm(name: String) -> Self {
        Modal::RemoveConfirm { project_name: name }
    }

    pub fn new_discover(paths: Vec<PathBuf>) -> Self {
        Modal::DiscoverResults { paths, selected: 0 }
    }

    pub fn new_config(interval: u64, concurrent: usize, air_gap: bool) -> Self {
        Modal::ConfigEditor {
            scan_interval: interval.to_string(),
            max_concurrent: concurrent.to_string(),
            air_gap,
            cursor_field: 0,
        }
    }

    pub fn new_help() -> Self {
        Modal::Help
    }

    /// Handle key input within the modal, returns Some(result) when modal should close
    pub fn handle_key(&mut self, key: KeyCode) -> Option<ModalResult> {
        match self {
            Modal::AddProject {
                path_input,
                name_input,
                tags_input,
                pin,
                cursor_field,
            } => match key {
                KeyCode::Esc => Some(ModalResult::Cancelled),
                KeyCode::Enter => {
                    if !path_input.is_empty() {
                        let path_clone = path_input.clone();
                        Some(ModalResult::AddProject {
                            path: PathBuf::from(path_clone.as_str()),
                            name: if name_input.is_empty() {
                                PathBuf::from(path_clone.as_str())
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| "unnamed".into())
                            } else {
                                name_input.clone()
                            },
                            tags: tags_input
                                .split(',')
                                .map(|t| t.trim().to_string())
                                .filter(|t| !t.is_empty())
                                .collect(),
                            pin: *pin,
                        })
                    } else {
                        None // Don't close if path is empty
                    }
                }
                KeyCode::Tab => {
                    *cursor_field = (*cursor_field + 1) % 3;
                    None
                }
                KeyCode::BackTab => {
                    *cursor_field = if *cursor_field == 0 {
                        2
                    } else {
                        *cursor_field - 1
                    };
                    None
                }
                KeyCode::Char(c) => {
                    let field = match cursor_field {
                        0 => path_input,
                        1 => name_input,
                        2 => tags_input,
                        _ => path_input,
                    };
                    field.push(c);
                    None
                }
                KeyCode::Backspace => {
                    let field = match cursor_field {
                        0 => path_input,
                        1 => name_input,
                        2 => tags_input,
                        _ => path_input,
                    };
                    field.pop();
                    None
                }
                _ => None,
            },

            Modal::RemoveConfirm { .. } => match key {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    Some(ModalResult::ConfirmRemove)
                }
                _ => Some(ModalResult::Cancelled),
            },

            Modal::DiscoverResults { paths, selected } => match key {
                KeyCode::Esc => Some(ModalResult::Cancelled),
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                    None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected < paths.len().saturating_sub(1) {
                        *selected += 1;
                    }
                    None
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    Some(ModalResult::SelectDiscovered(paths[*selected].clone()))
                }
                KeyCode::Char('a') => Some(ModalResult::SelectAllDiscovered(paths.clone())),
                _ => None,
            },

            Modal::ConfigEditor {
                scan_interval,
                max_concurrent,
                air_gap,
                cursor_field,
            } => match key {
                KeyCode::Esc => Some(ModalResult::Cancelled),
                KeyCode::Enter => Some(ModalResult::SaveConfig {
                    scan_interval: scan_interval.parse().unwrap_or(300),
                    max_concurrent: max_concurrent.parse().unwrap_or(8),
                    air_gap: *air_gap,
                }),
                KeyCode::Tab => {
                    *cursor_field = (*cursor_field + 1) % 3;
                    None
                }
                KeyCode::Char(c) => match cursor_field {
                    0 => {
                        if c.is_ascii_digit() {
                            scan_interval.push(c);
                        }
                        None
                    }
                    1 => {
                        if c.is_ascii_digit() {
                            max_concurrent.push(c);
                        }
                        None
                    }
                    2 => {
                        *air_gap = !*air_gap;
                        None
                    }
                    _ => None,
                },
                KeyCode::Backspace => match cursor_field {
                    0 => {
                        scan_interval.pop();
                        None
                    }
                    1 => {
                        max_concurrent.pop();
                        None
                    }
                    _ => None,
                },
                _ => None,
            },

            Modal::Help => match key {
                _ => Some(ModalResult::Cancelled),
            },

            Modal::ProjectDetail { .. } => match key {
                _ => Some(ModalResult::Cancelled),
            },
        }
    }
}

/// Result of a completed modal interaction
#[derive(Debug, Clone)]
pub enum ModalResult {
    Cancelled,
    AddProject {
        path: PathBuf,
        name: String,
        tags: Vec<String>,
        pin: bool,
    },
    ConfirmRemove,
    SelectDiscovered(PathBuf),
    SelectAllDiscovered(Vec<PathBuf>),
    SaveConfig {
        scan_interval: u64,
        max_concurrent: usize,
        air_gap: bool,
    },
}

/// Draw a centered modal overlay on the frame
pub fn draw_modal(frame: &mut Frame, modal: &Modal) {
    let area = centered_rect(60, 70, frame.area());

    // Clear background
    frame.render_widget(Clear, area);

    match modal {
        Modal::AddProject {
            path_input,
            name_input,
            tags_input,
            pin,
            cursor_field,
        } => {
            let block = Block::default()
                .title(" Add Project ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            let fields = vec![
                Line::from(vec![
                    Span::styled(
                        " Path:   ",
                        Style::default()
                            .fg(if *cursor_field == 0 {
                                Color::Yellow
                            } else {
                                Color::White
                            })
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        path_input.as_str(),
                        Style::default().fg(if *cursor_field == 0 {
                            Color::Green
                        } else {
                            Color::White
                        }),
                    ),
                    if *cursor_field == 0 {
                        Span::styled("█", Style::default().fg(Color::Yellow))
                    } else {
                        Span::raw("")
                    },
                ]),
                Line::from(vec![
                    Span::styled(
                        " Name:   ",
                        Style::default()
                            .fg(if *cursor_field == 1 {
                                Color::Yellow
                            } else {
                                Color::White
                            })
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        name_input.as_str(),
                        Style::default().fg(if *cursor_field == 1 {
                            Color::Green
                        } else {
                            Color::White
                        }),
                    ),
                    if *cursor_field == 1 {
                        Span::styled("█", Style::default().fg(Color::Yellow))
                    } else {
                        Span::raw("")
                    },
                ]),
                Line::from(vec![
                    Span::styled(
                        " Tags:   ",
                        Style::default()
                            .fg(if *cursor_field == 2 {
                                Color::Yellow
                            } else {
                                Color::White
                            })
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        tags_input.as_str(),
                        Style::default().fg(if *cursor_field == 2 {
                            Color::Green
                        } else {
                            Color::White
                        }),
                    ),
                    if *cursor_field == 2 {
                        Span::styled("█", Style::default().fg(Color::Yellow))
                    } else {
                        Span::raw("")
                    },
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::raw(" Pin: "),
                    Span::styled(
                        if *pin { "[✓]" } else { "[ ]" },
                        Style::default().fg(Color::Magenta),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        "Tab",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(":next  "),
                    Span::styled(
                        "Enter",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(":confirm  "),
                    Span::styled(
                        "Esc",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(":cancel"),
                ]),
            ];

            let paragraph = Paragraph::new(fields)
                .block(block)
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: false });
            frame.render_widget(paragraph, area);
        }

        Modal::RemoveConfirm { project_name } => {
            let block = Block::default()
                .title(" Confirm Removal ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red));

            let text = vec![
                Line::from(""),
                Line::from(vec![
                    Span::raw("Remove project "),
                    Span::styled(
                        project_name.as_str(),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("?"),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        "Y",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("es / "),
                    Span::styled(
                        "N",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("o"),
                ]),
            ];

            let paragraph = Paragraph::new(text)
                .block(block)
                .alignment(Alignment::Center);
            frame.render_widget(paragraph, area);
        }

        Modal::DiscoverResults { paths, selected } => {
            let block = Block::default()
                .title(format!(" Discovered Projects ({}) ", paths.len()))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green));

            let items: Vec<Line> = paths
                .iter()
                .enumerate()
                .take(area.height as usize - 4)
                .map(|(i, p)| {
                    let marker = if i == *selected { "▸ " } else { "  " };
                    let style = if i == *selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(Span::styled(format!("{}{}", marker, p.display()), style))
                })
                .collect();

            let mut lines = items;
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("j/k", Style::default().fg(Color::Yellow)),
                Span::raw(":nav  "),
                Span::styled("Enter", Style::default().fg(Color::Green)),
                Span::raw(":select  "),
                Span::styled("a", Style::default().fg(Color::Cyan)),
                Span::raw(":all  "),
                Span::styled("Esc", Style::default().fg(Color::Red)),
                Span::raw(":cancel"),
            ]));

            let paragraph = Paragraph::new(lines).block(block);
            frame.render_widget(paragraph, area);
        }

        Modal::ConfigEditor {
            scan_interval,
            max_concurrent,
            air_gap,
            cursor_field,
        } => {
            let block = Block::default()
                .title(" Configuration ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            let fields = vec![
                Line::from(vec![
                    Span::styled(
                        " Scan interval (s): ",
                        Style::default()
                            .fg(if *cursor_field == 0 {
                                Color::Yellow
                            } else {
                                Color::White
                            })
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        scan_interval.as_str(),
                        Style::default().fg(if *cursor_field == 0 {
                            Color::Green
                        } else {
                            Color::White
                        }),
                    ),
                    if *cursor_field == 0 {
                        Span::styled("█", Style::default().fg(Color::Yellow))
                    } else {
                        Span::raw("")
                    },
                ]),
                Line::from(vec![
                    Span::styled(
                        " Max concurrent:    ",
                        Style::default()
                            .fg(if *cursor_field == 1 {
                                Color::Yellow
                            } else {
                                Color::White
                            })
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        max_concurrent.as_str(),
                        Style::default().fg(if *cursor_field == 1 {
                            Color::Green
                        } else {
                            Color::White
                        }),
                    ),
                    if *cursor_field == 1 {
                        Span::styled("█", Style::default().fg(Color::Yellow))
                    } else {
                        Span::raw("")
                    },
                ]),
                Line::from(vec![
                    Span::styled(
                        " Air-gap mode:      ",
                        Style::default()
                            .fg(if *cursor_field == 2 {
                                Color::Yellow
                            } else {
                                Color::White
                            })
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        if *air_gap { "[✓] ON " } else { "[ ] OFF" },
                        Style::default().fg(if *air_gap { Color::Red } else { Color::Green }),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Enter", Style::default().fg(Color::Green)),
                    Span::raw(":save  "),
                    Span::styled("Esc", Style::default().fg(Color::Red)),
                    Span::raw(":cancel"),
                ]),
            ];

            let paragraph = Paragraph::new(fields).block(block);
            frame.render_widget(paragraph, area);
        }

        Modal::Help => {
            let block = Block::default()
                .title(" Keyboard Shortcuts ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            let shortcuts = vec![
                ("j / ↓", "Move down"),
                ("k / ↑", "Move up"),
                ("g / Home", "Go to top"),
                ("G / End", "Go to bottom"),
                ("Enter", "Project details"),
                ("a", "Add project"),
                ("d", "Delete project"),
                ("f", "Find/discover projects"),
                ("p", "Pin/unpin project"),
                ("r", "Refresh scan"),
                ("s", "Start daemon"),
                ("x", "Stop daemon"),
                ("t", "Toggle auto-refresh"),
                ("c", "Configuration"),
                ("? / h", "This help"),
                ("q / Esc", "Quit"),
            ];

            let lines: Vec<Line> = shortcuts
                .iter()
                .map(|(key, desc)| {
                    Line::from(vec![
                        Span::styled(
                            format!("{:<12}", key),
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(*desc, Style::default().fg(Color::White)),
                    ])
                })
                .collect();

            let paragraph = Paragraph::new(lines).block(block);
            frame.render_widget(paragraph, area);
        }

        Modal::ProjectDetail {
            name,
            branch,
            dirty,
            ahead,
            behind,
            deps_total,
            deps_outdated,
            deps_security,
        } => {
            let block = Block::default()
                .title(format!(" {} ", name))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan));

            let dirty_str = if *dirty {
                Span::styled(" YES ⚠", Style::default().fg(Color::Red))
            } else {
                Span::styled(" no", Style::default().fg(Color::Green))
            };

            let lines = vec![
                Line::from(vec![
                    Span::styled(" Branch:   ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(branch.as_str(), Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    Span::styled(" Dirty:    ", Style::default().add_modifier(Modifier::BOLD)),
                    dirty_str,
                ]),
                Line::from(vec![
                    Span::styled(" Ahead:    ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(ahead.to_string(), Style::default().fg(Color::Green)),
                ]),
                Line::from(vec![
                    Span::styled(" Behind:   ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(behind.to_string(), Style::default().fg(Color::Yellow)),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled(
                    " Dependencies:",
                    Style::default().add_modifier(Modifier::BOLD),
                )]),
                Line::from(vec![
                    Span::raw("   Total:    "),
                    Span::styled(deps_total.to_string(), Style::default().fg(Color::White)),
                ]),
                Line::from(vec![
                    Span::raw("   Outdated: "),
                    Span::styled(
                        deps_outdated.to_string(),
                        Style::default().fg(if *deps_outdated > 0 {
                            Color::Yellow
                        } else {
                            Color::Green
                        }),
                    ),
                ]),
                Line::from(vec![
                    Span::raw("   Security: "),
                    Span::styled(
                        deps_security.to_string(),
                        Style::default().fg(if *deps_security > 0 {
                            Color::Red
                        } else {
                            Color::Green
                        }),
                    ),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "Press any key to close",
                    Style::default().fg(Color::DarkGray),
                )),
            ];

            let paragraph = Paragraph::new(lines).block(block);
            frame.render_widget(paragraph, area);
        }
    }
}

/// Calculate a centered rectangle area
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

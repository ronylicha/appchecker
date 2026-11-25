use ratatui::style::{Color, Modifier, Style};

pub const ACCENT: Color = Color::Cyan;
pub const TEXT: Color = Color::White;
pub const MUTED: Color = Color::DarkGray;
pub const SUCCESS: Color = Color::Green;
pub const WARNING: Color = Color::Yellow;
pub const ERROR: Color = Color::Red;
pub const PANEL: Color = Color::Rgb(30, 41, 59);
pub const SELECTED: Color = Color::Rgb(51, 65, 85);

pub fn title() -> Style {
    Style::default().fg(TEXT).add_modifier(Modifier::BOLD)
}

pub fn muted() -> Style {
    Style::default().fg(MUTED)
}

pub fn action() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn selected() -> Style {
    Style::default().bg(SELECTED).fg(TEXT)
}

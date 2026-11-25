use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

/// Represents user actions parsed from terminal input
#[derive(Debug, Clone, PartialEq)]
pub enum UserAction {
    // Navigation
    MoveUp,
    MoveDown,
    MoveToTop,
    MoveToBottom,
    SwitchTab(usize),

    // Project management
    AddProject,
    RemoveProject,
    DiscoverProjects,
    PinProject,
    RefreshProject,
    OpenProjectDetail,

    // Daemon control
    StartDaemon,
    StopDaemon,
    ToggleAutoRefresh,

    // Config
    OpenConfig,
    ChangeScanInterval,
    ToggleNotifications,

    // UI
    ShowHelp,
    Quit,
    None,
}

/// Parse terminal events into high-level user actions
pub fn parse_input(timeout: Duration) -> Option<UserAction> {
    if !event::poll(timeout).ok()? {
        return Some(UserAction::None);
    }

    match event::read().ok()? {
        Event::Key(key) => Some(parse_key(key)),
        _ => Some(UserAction::None),
    }
}

fn parse_key(key: KeyEvent) -> UserAction {
    match (key.modifiers, key.code) {
        // Quit
        (_, KeyCode::Char('q')) => UserAction::Quit,
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => UserAction::Quit,

        // Navigation
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => UserAction::MoveUp,
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => UserAction::MoveDown,
        (_, KeyCode::Home) | (_, KeyCode::Char('g')) => UserAction::MoveToTop,
        (_, KeyCode::End) | (_, KeyCode::Char('G')) => UserAction::MoveToBottom,

        // Tabs (1-4)
        (_, KeyCode::Char('1')) => UserAction::SwitchTab(0),
        (_, KeyCode::Char('2')) => UserAction::SwitchTab(1),
        (_, KeyCode::Char('3')) => UserAction::SwitchTab(2),
        (_, KeyCode::Char('4')) => UserAction::SwitchTab(3),

        // Project actions
        (_, KeyCode::Char('a')) => UserAction::AddProject,
        (_, KeyCode::Char('d')) => UserAction::RemoveProject,
        (_, KeyCode::Char('f')) => UserAction::DiscoverProjects,
        (_, KeyCode::Char('p')) => UserAction::PinProject,
        (_, KeyCode::Char('r')) => UserAction::RefreshProject,
        (_, KeyCode::Enter) => UserAction::OpenProjectDetail,

        // Daemon
        (_, KeyCode::Char('s')) => UserAction::StartDaemon,
        (_, KeyCode::Char('x')) => UserAction::StopDaemon,
        (_, KeyCode::Char('t')) => UserAction::ToggleAutoRefresh,

        // Config & help
        (_, KeyCode::Char('c')) => UserAction::OpenConfig,
        (_, KeyCode::Char('?')) | (_, KeyCode::Char('h')) => UserAction::ShowHelp,

        _ => UserAction::None,
    }
}

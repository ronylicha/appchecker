use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserAction {
    MoveUp,
    MoveDown,
    MoveToTop,
    MoveToBottom,
    SwitchView(usize),
    ShowDependencies,
    AddProject,
    EditProject,
    RemoveProject,
    DiscoverProjects,
    PinProject,
    RefreshProject,
    RefreshAll,
    UpdateDependency,
    UpdateAll,
    OpenConfig,
    ShowHelp,
    Quit,
    Close,
    None,
}

pub fn parse_key(key: KeyEvent) -> UserAction {
    match (key.modifiers, key.code) {
        (_, KeyCode::Char('q')) => UserAction::Quit,
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => UserAction::Quit,
        (_, KeyCode::Esc) => UserAction::Close,
        (_, KeyCode::Up) | (_, KeyCode::Char('k')) => UserAction::MoveUp,
        (_, KeyCode::Down) | (_, KeyCode::Char('j')) => UserAction::MoveDown,
        (_, KeyCode::Home) | (_, KeyCode::Char('g')) => UserAction::MoveToTop,
        (_, KeyCode::End) | (_, KeyCode::Char('G')) => UserAction::MoveToBottom,
        (_, KeyCode::Char('1')) => UserAction::SwitchView(0),
        (_, KeyCode::Char('2')) => UserAction::SwitchView(1),
        (_, KeyCode::Char('3')) => UserAction::SwitchView(2),
        (_, KeyCode::Enter) => UserAction::ShowDependencies,
        (_, KeyCode::Char('a')) => UserAction::AddProject,
        (_, KeyCode::Char('e')) => UserAction::EditProject,
        (_, KeyCode::Char('d')) => UserAction::RemoveProject,
        (_, KeyCode::Char('f')) => UserAction::DiscoverProjects,
        (_, KeyCode::Char('p')) => UserAction::PinProject,
        (_, KeyCode::Char('r')) => UserAction::RefreshProject,
        (_, KeyCode::Char('R')) => UserAction::RefreshAll,
        (_, KeyCode::Char('u')) => UserAction::UpdateDependency,
        (_, KeyCode::Char('U')) => UserAction::UpdateAll,
        (_, KeyCode::Char('c')) => UserAction::OpenConfig,
        (_, KeyCode::Char('?')) | (_, KeyCode::Char('h')) => UserAction::ShowHelp,
        _ => UserAction::None,
    }
}

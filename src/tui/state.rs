use crate::config::ProjectDraft;
use crate::core::{MAX_CONCURRENT_SCANS, Project};
use crossterm::event::{KeyCode, KeyEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Projects,
    Dependencies,
    Activity,
}

impl View {
    pub fn index(self) -> usize {
        match self {
            Self::Projects => 0,
            Self::Dependencies => 1,
            Self::Activity => 2,
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Dependencies,
            2 => Self::Activity,
            _ => Self::Projects,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormEvent {
    Continue,
    Submit(ProjectDraft),
    Cancel,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectForm {
    pub path: String,
    pub name: String,
    pub tags: String,
    pub pinned: bool,
    pub field: usize,
}

impl ProjectForm {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_project(project: &Project) -> Self {
        Self {
            path: project.path.to_string_lossy().to_string(),
            name: project.name.clone(),
            tags: project.tags.join(", "),
            pinned: project.pinned,
            field: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> FormEvent {
        match key.code {
            KeyCode::Esc => FormEvent::Cancel,
            KeyCode::Tab => {
                self.field = (self.field + 1) % 4;
                FormEvent::Continue
            }
            KeyCode::BackTab => {
                self.field = self.field.checked_sub(1).unwrap_or(3);
                FormEvent::Continue
            }
            KeyCode::Enter => FormEvent::Submit(ProjectDraft {
                path: self.path.clone().into(),
                name: self.name.clone(),
                tags: self
                    .tags
                    .split(',')
                    .map(|tag| tag.trim().to_string())
                    .filter(|tag| !tag.is_empty())
                    .collect(),
                pinned: self.pinned,
            }),
            KeyCode::Char(' ') if self.field == 3 => {
                self.pinned = !self.pinned;
                FormEvent::Continue
            }
            KeyCode::Char(character) => {
                if let Some(field) = self.text_field_mut() {
                    field.push(character);
                }
                FormEvent::Continue
            }
            KeyCode::Backspace => {
                if let Some(field) = self.text_field_mut() {
                    field.pop();
                }
                FormEvent::Continue
            }
            _ => FormEvent::Continue,
        }
    }

    fn text_field_mut(&mut self) -> Option<&mut String> {
        match self.field {
            0 => Some(&mut self.path),
            1 => Some(&mut self.name),
            2 => Some(&mut self.tags),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigEvent {
    Continue,
    Submit {
        scan_interval: u64,
        max_concurrent: usize,
        air_gap: bool,
    },
    Error(String),
    Cancel,
}

#[derive(Debug, Clone)]
pub struct ConfigForm {
    pub scan_interval: String,
    pub max_concurrent: String,
    pub air_gap: bool,
    pub field: usize,
}

impl ConfigForm {
    pub fn new(scan_interval: u64, max_concurrent: usize, air_gap: bool) -> Self {
        Self {
            scan_interval: scan_interval.to_string(),
            max_concurrent: max_concurrent.to_string(),
            air_gap,
            field: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> ConfigEvent {
        match key.code {
            KeyCode::Esc => ConfigEvent::Cancel,
            KeyCode::Tab => {
                self.field = (self.field + 1) % 3;
                ConfigEvent::Continue
            }
            KeyCode::BackTab => {
                self.field = self.field.checked_sub(1).unwrap_or(2);
                ConfigEvent::Continue
            }
            KeyCode::Enter => {
                let scan_interval = self.scan_interval.parse::<u64>().unwrap_or(0);
                let max_concurrent = self.max_concurrent.parse::<usize>().unwrap_or(0);
                if scan_interval == 0 {
                    return ConfigEvent::Error("Scan interval must be greater than zero".into());
                }
                if !(1..=MAX_CONCURRENT_SCANS).contains(&max_concurrent) {
                    return ConfigEvent::Error(format!(
                        "Concurrent scans must be between 1 and {MAX_CONCURRENT_SCANS}"
                    ));
                }
                ConfigEvent::Submit {
                    scan_interval,
                    max_concurrent,
                    air_gap: self.air_gap,
                }
            }
            KeyCode::Char(' ') if self.field == 2 => {
                self.air_gap = !self.air_gap;
                ConfigEvent::Continue
            }
            KeyCode::Char(character) if character.is_ascii_digit() => {
                match self.field {
                    0 => self.scan_interval.push(character),
                    1 => self.max_concurrent.push(character),
                    _ => {}
                }
                ConfigEvent::Continue
            }
            KeyCode::Backspace => {
                match self.field {
                    0 => {
                        self.scan_interval.pop();
                    }
                    1 => {
                        self.max_concurrent.pop();
                    }
                    _ => {}
                }
                ConfigEvent::Continue
            }
            _ => ConfigEvent::Continue,
        }
    }
}

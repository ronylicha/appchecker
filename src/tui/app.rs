use crate::core::{AppConfig, ProjectHealth};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::sync::{Arc, Mutex};
use tracing::info;

use super::ui;

/// Main TUI application state and event loop.
pub struct TuiApp {
    config: Arc<Mutex<AppConfig>>,
    health_data: Arc<Mutex<Vec<ProjectHealth>>>,
    selected_index: usize,
    should_quit: bool,
}

impl TuiApp {
    pub fn new(config: AppConfig, health_data: Vec<ProjectHealth>) -> Self {
        Self {
            config: Arc::new(Mutex::new(config)),
            health_data: Arc::new(Mutex::new(health_data)),
            selected_index: 0,
            should_quit: false,
        }
    }

    /// Run the TUI event loop until the user quits.
    pub fn run(&mut self) -> anyhow::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        info!("TUI started");

        loop {
            // Draw UI
            let health = self.health_data.lock().unwrap().clone();
            let config = self.config.lock().unwrap().clone();
            terminal.draw(|frame| {
                ui::draw(frame, &health, &config, self.selected_index);
            })?;

            // Handle input with timeout for periodic refresh
            if event::poll(std::time::Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            self.should_quit = true;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if self.selected_index > 0 {
                                self.selected_index -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            let len = health.len();
                            if len > 0 && self.selected_index < len - 1 {
                                self.selected_index += 1;
                            }
                        }
                        KeyCode::Home | KeyCode::Char('g') => {
                            self.selected_index = 0;
                        }
                        KeyCode::End | KeyCode::Char('G') => {
                            let len = health.len();
                            if len > 0 {
                                self.selected_index = len - 1;
                            }
                        }
                        KeyCode::Char('p') => {
                            // Toggle pin on selected project
                            let health = self.health_data.lock().unwrap();
                            if let Some(h) = health.get(self.selected_index) {
                                let mut config = self.config.lock().unwrap();
                                if let Some(proj) =
                                    config.projects.iter_mut().find(|p| p.id == h.project_id)
                                {
                                    proj.pinned = !proj.pinned;
                                }
                            }
                        }
                        KeyCode::Char('r') => {
                            // Trigger manual rescan (placeholder)
                            info!("Manual rescan triggered");
                        }
                        _ => {}
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        info!("TUI exited");
        Ok(())
    }
}

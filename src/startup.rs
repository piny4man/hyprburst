use ratatui::crossterm::event::KeyCode;
use ratatui::prelude::*;

use crate::config::Config;
use crate::desktop::DesktopEntry;
use crate::launcher::Launcher;
use crate::layout::{self, LayoutRects};

pub enum StartupState {
    Loading(StartupShell),
    Loaded(Launcher),
    Empty(StartupShell),
    Failed(StartupShell),
}

pub struct StartupShell {
    config: Config,
    query: String,
    running: bool,
    status: StartupStatus,
}

enum StartupStatus {
    Loading,
    Empty,
    Failed(String),
}

impl StartupState {
    pub fn discover(config: Config) -> Self {
        let launcher = Launcher::new(config.clone());
        if launcher.apps.is_empty() {
            Self::empty(config, launcher.query.clone())
        } else {
            Self::Loaded(launcher)
        }
    }

    pub fn loading(config: Config) -> Self {
        Self::Loading(StartupShell::new(
            config,
            String::new(),
            true,
            StartupStatus::Loading,
        ))
    }

    pub fn loaded(config: Config, apps: Vec<DesktopEntry>, query: String) -> Self {
        let launcher = Launcher::from_apps(config, apps, query);
        if launcher.apps.is_empty() {
            Self::Empty(StartupShell::new(
                launcher.config.clone(),
                launcher.query.clone(),
                false,
                StartupStatus::Empty,
            ))
        } else {
            Self::Loaded(launcher)
        }
    }

    pub fn empty(config: Config, query: String) -> Self {
        Self::Empty(StartupShell::new(
            config,
            query,
            false,
            StartupStatus::Empty,
        ))
    }

    pub fn failed(config: Config, query: String, error: String) -> Self {
        Self::Failed(StartupShell::new(
            config,
            query,
            false,
            StartupStatus::Failed(error),
        ))
    }

    pub fn handle_key(&mut self, code: KeyCode) {
        match self {
            Self::Loaded(launcher) => launcher.handle_key(code),
            Self::Loading(shell) | Self::Empty(shell) | Self::Failed(shell) => {
                shell.handle_key(code);
            }
        }
    }

    pub fn is_running(&self) -> bool {
        match self {
            Self::Loaded(launcher) => launcher.running,
            Self::Loading(shell) | Self::Empty(shell) | Self::Failed(shell) => shell.running,
        }
    }

    pub fn query(&self) -> &str {
        match self {
            Self::Loaded(launcher) => &launcher.query,
            Self::Loading(shell) | Self::Empty(shell) | Self::Failed(shell) => &shell.query,
        }
    }
}

impl StartupShell {
    fn new(config: Config, query: String, running: bool, status: StartupStatus) -> Self {
        Self {
            config,
            query,
            running,
            status,
        }
    }

    fn handle_key(&mut self, code: KeyCode) {
        if !self.running {
            return;
        }

        match code {
            KeyCode::Esc => self.running = false,
            KeyCode::Backspace => {
                self.query.pop();
            }
            KeyCode::Char(c) => self.query.push(c),
            _ => {}
        }
    }

    fn status_message(&self) -> &str {
        match &self.status {
            StartupStatus::Loading => "Loading applications...",
            StartupStatus::Empty => "No applications found",
            StartupStatus::Failed(_) => "Failed to discover applications",
        }
    }
}

impl Widget for &mut StartupState {
    fn render(self, area: Rect, buf: &mut Buffer) {
        match self {
            StartupState::Loaded(launcher) => launcher.render(area, buf),
            StartupState::Loading(shell)
            | StartupState::Empty(shell)
            | StartupState::Failed(shell) => {
                shell.render(area, buf);
            }
        }
    }
}

impl Widget for &mut StartupShell {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let LayoutRects {
            banner: banner_area,
            input: input_area,
            separator: separator_area,
            list: list_area,
            columns: _,
        } = layout::compute(area, &self.config);

        if banner_area.height > 0 && !self.config.ui.banner.is_empty() {
            let banner_style = Style::new()
                .fg(self.config.colors.banner)
                .add_modifier(Modifier::BOLD);
            for (i, line) in self
                .config
                .ui
                .banner
                .lines()
                .take(banner_area.height as usize)
                .enumerate()
            {
                buf.set_string(banner_area.x, banner_area.y + i as u16, line, banner_style);
            }
        }

        if input_area.height == 0 {
            return;
        }

        let prompt_text = format!("{}{}", self.config.ui.prompt, self.query);
        let input_style = Style::new()
            .fg(self.config.colors.prompt)
            .add_modifier(Modifier::BOLD);
        buf.set_string(input_area.x, input_area.y, &prompt_text, input_style);

        if self.config.ui.show_cursor {
            let cursor_x = input_area.x + prompt_text.chars().count() as u16;
            if cursor_x < input_area.x + input_area.width {
                buf.set_string(
                    cursor_x,
                    input_area.y,
                    &self.config.ui.cursor_char,
                    Style::new().fg(self.config.colors.prompt),
                );
            }
        }

        if let Some(sep) = separator_area {
            let sep_line: String = "─".repeat(sep.width as usize);
            buf.set_string(
                sep.x,
                sep.y,
                &sep_line,
                Style::new().fg(self.config.colors.prompt),
            );
        }

        if list_area.height == 0 {
            return;
        }

        let style = Style::new().fg(self.config.colors.empty);
        buf.set_string(list_area.x, list_area.y, self.status_message(), style);
        if let StartupStatus::Failed(error) = &self.status
            && list_area.height > 1
        {
            buf.set_string(list_area.x, list_area.y + 1, error, style);
        }
    }
}

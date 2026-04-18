//! Keyboard input polling via crossterm.

use std::io;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

pub fn poll_key() -> io::Result<Option<KeyCode>> {
    if !event::poll(std::time::Duration::from_millis(16))? {
        return Ok(None);
    }
    match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Press => Ok(Some(key.code)),
        _ => Ok(None),
    }
}

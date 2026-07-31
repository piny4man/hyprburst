//! Keyboard and mouse input polling via crossterm.

use std::io;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEvent};

pub enum Input {
    Key(KeyCode),
    Mouse(MouseEvent),
}

pub fn poll() -> io::Result<Option<Input>> {
    if !event::poll(std::time::Duration::from_millis(16))? {
        return Ok(None);
    }
    match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Press => Ok(Some(Input::Key(key.code))),
        Event::Mouse(mouse) => Ok(Some(Input::Mouse(mouse))),
        _ => Ok(None),
    }
}

//! Keyboard and mouse input polling via crossterm.

use std::io;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEvent};

pub enum Input {
    Key(KeyCode),
    Mouse(MouseEvent),
}

/// Block up to one frame (16 ms) for the first event, then drain everything
/// already queued without blocking. Returns every pending input so the caller
/// can apply a burst (key repeat, paste) and redraw once. An empty batch means
/// the frame elapsed idle — nothing to redraw.
pub fn poll_batch() -> io::Result<Vec<Input>> {
    let mut batch = Vec::new();
    if !event::poll(std::time::Duration::from_millis(16))? {
        return Ok(batch);
    }
    loop {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                batch.push(Input::Key(key.code));
            }
            Event::Mouse(mouse) => batch.push(Input::Mouse(mouse)),
            _ => {}
        }
        // Drain the rest of the queue non-blocking.
        if !event::poll(std::time::Duration::ZERO)? {
            break;
        }
    }
    Ok(batch)
}

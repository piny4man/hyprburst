//! Keyboard, mouse, and resize input polling via crossterm.

use std::io;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseEvent};

pub enum Input {
    Key(KeyCode),
    Mouse(MouseEvent),
    /// The terminal changed size; the next draw lays out the new grid.
    Resize,
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
        batch.extend(to_input(event::read()?));
        // Drain the rest of the queue non-blocking.
        if !event::poll(std::time::Duration::ZERO)? {
            break;
        }
    }
    Ok(batch)
}

/// The input a terminal event carries, if any.
fn to_input(event: Event) -> Option<Input> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => Some(Input::Key(key.code)),
        Event::Mouse(mouse) => Some(Input::Mouse(mouse)),
        Event::Resize(..) => Some(Input::Resize),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEvent, KeyEventKind, KeyModifiers};

    #[test]
    fn a_resize_is_an_input_so_the_grid_is_redrawn() {
        assert!(matches!(
            to_input(Event::Resize(120, 40)),
            Some(Input::Resize)
        ));
    }

    #[test]
    fn only_key_presses_are_inputs() {
        let press = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert!(matches!(
            to_input(Event::Key(press)),
            Some(Input::Key(KeyCode::Char('a')))
        ));
        assert!(to_input(Event::Key(release)).is_none());
        assert!(to_input(Event::FocusGained).is_none());
    }
}

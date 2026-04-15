use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};

pub fn poll_event() -> io::Result<Option<Event>> {
    if event::poll(std::time::Duration::from_millis(16))? {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                return Ok(Some(Event::Key(key)));
            }
            other => return Ok(Some(other)),
        }
    }
    Ok(None)
}

pub fn is_escape(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(key) if key.code == KeyCode::Esc
    )
}

pub fn is_up(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(key) if key.code == KeyCode::Up
    )
}

pub fn is_down(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(key) if key.code == KeyCode::Down
    )
}

pub fn is_enter(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(key) if key.code == KeyCode::Enter
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventState, KeyModifiers};

    fn make_key_event(code: KeyCode) -> Event {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    }

    #[test]
    fn is_escape_detects_esc() {
        let event = make_key_event(KeyCode::Esc);
        assert!(is_escape(&event));
    }

    #[test]
    fn is_escape_rejects_other_keys() {
        for code in [
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Char('a'),
            KeyCode::Char('q'),
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
        ] {
            let event = make_key_event(code);
            assert!(!is_escape(&event), "{:?} should not be escape", code);
        }
    }

    #[test]
    fn is_up_detects_up_arrow() {
        let event = make_key_event(KeyCode::Up);
        assert!(is_up(&event));
    }

    #[test]
    fn is_up_rejects_other_keys() {
        for code in [
            KeyCode::Esc,
            KeyCode::Enter,
            KeyCode::Down,
            KeyCode::Char('a'),
        ] {
            let event = make_key_event(code);
            assert!(!is_up(&event), "{:?} should not be up", code);
        }
    }

    #[test]
    fn is_down_detects_down_arrow() {
        let event = make_key_event(KeyCode::Down);
        assert!(is_down(&event));
    }

    #[test]
    fn is_down_rejects_other_keys() {
        for code in [
            KeyCode::Esc,
            KeyCode::Enter,
            KeyCode::Up,
            KeyCode::Char('a'),
        ] {
            let event = make_key_event(code);
            assert!(!is_down(&event), "{:?} should not be down", code);
        }
    }

    #[test]
    fn is_enter_detects_enter() {
        let event = make_key_event(KeyCode::Enter);
        assert!(is_enter(&event));
    }

    #[test]
    fn is_enter_rejects_other_keys() {
        for code in [
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Char('a'),
        ] {
            let event = make_key_event(code);
            assert!(!is_enter(&event), "{:?} should not be enter", code);
        }
    }
}

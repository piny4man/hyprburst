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

pub fn is_tab(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(key) if key.code == KeyCode::Tab
    )
}

pub fn is_page_up(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(key) if key.code == KeyCode::PageUp
    )
}

pub fn is_page_down(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(key) if key.code == KeyCode::PageDown
    )
}

pub fn is_backspace(event: &Event) -> bool {
    matches!(
        event,
        Event::Key(key) if key.code == KeyCode::Backspace
    )
}

pub fn char_from_event(event: &Event) -> Option<char> {
    match event {
        Event::Key(key) if matches!(key.code, KeyCode::Char(_)) => {
            if let KeyCode::Char(c) = key.code {
                Some(c)
            } else {
                None
            }
        }
        _ => None,
    }
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

    #[test]
    fn is_tab_detects_tab() {
        let event = make_key_event(KeyCode::Tab);
        assert!(is_tab(&event));
    }

    #[test]
    fn is_page_up_detects_page_up() {
        let event = make_key_event(KeyCode::PageUp);
        assert!(is_page_up(&event));
    }

    #[test]
    fn is_page_down_detects_page_down() {
        let event = make_key_event(KeyCode::PageDown);
        assert!(is_page_down(&event));
    }

    #[test]
    fn is_backspace_detects_backspace() {
        let event = make_key_event(KeyCode::Backspace);
        assert!(is_backspace(&event));
    }

    #[test]
    fn char_from_event_returns_char() {
        let event = make_key_event(KeyCode::Char('a'));
        assert_eq!(char_from_event(&event), Some('a'));
    }

    #[test]
    fn char_from_event_returns_none_for_non_char() {
        for code in [
            KeyCode::Esc,
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Up,
            KeyCode::Down,
        ] {
            let event = make_key_event(code);
            assert!(
                char_from_event(&event).is_none(),
                "{:?} should not produce a char",
                code
            );
        }
    }
}

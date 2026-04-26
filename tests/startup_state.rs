use burst::config::{Config, UiConfig};
use burst::desktop::DesktopEntry;
use burst::startup::StartupState;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

fn config() -> Config {
    Config {
        ui: UiConfig {
            banner: String::new(),
            ..UiConfig::default()
        },
        ..Config::default()
    }
}

fn app(name: &str) -> DesktopEntry {
    DesktopEntry {
        id: name.to_lowercase(),
        name: name.to_string(),
        icon: String::new(),
        exec: name.to_lowercase(),
    }
}

fn render(state: &mut StartupState, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    state.render(area, &mut buf);
    buf
}

fn frame_contains(buf: &Buffer, needle: &str) -> bool {
    let area = *buf.area();
    (0..area.height).any(|y| {
        let row: String = (0..area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        row.contains(needle)
    })
}

#[test]
fn startup_state_preserves_query_while_loading() {
    let mut state = StartupState::loading(config());

    state.handle_key(KeyCode::Char('f'));
    state.handle_key(KeyCode::Char('i'));

    assert_eq!(state.query(), "fi");
    assert!(state.is_running());
}

#[test]
fn escape_stops_loading_state() {
    let mut state = StartupState::loading(config());

    state.handle_key(KeyCode::Esc);

    assert!(!state.is_running());
}

#[test]
fn buffer_rendering_covers_loading_loaded_empty_and_error_states() {
    let mut loading = StartupState::loading(config());
    loading.handle_key(KeyCode::Char('f'));
    assert!(frame_contains(&render(&mut loading, 40, 7), "> f"));
    assert!(frame_contains(
        &render(&mut loading, 40, 7),
        "Loading applications"
    ));

    let mut loaded = StartupState::loaded(config(), vec![app("Firefox")], "fire".to_string());
    assert!(frame_contains(&render(&mut loaded, 40, 7), "> fire"));
    assert!(frame_contains(&render(&mut loaded, 40, 7), "Firefox"));

    let mut empty = StartupState::empty(config(), "abc".to_string());
    assert!(frame_contains(&render(&mut empty, 40, 7), "> abc"));
    assert!(frame_contains(
        &render(&mut empty, 40, 7),
        "No applications found"
    ));

    let mut failed =
        StartupState::failed(config(), "abc".to_string(), "permission denied".to_string());
    assert!(frame_contains(&render(&mut failed, 60, 7), "> abc"));
    assert!(frame_contains(
        &render(&mut failed, 60, 7),
        "Failed to discover applications"
    ));
}

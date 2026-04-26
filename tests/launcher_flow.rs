//! End-to-end integration tests that drive the full `App` through key events and
//! render it to a `Buffer`, asserting on the resulting frame just like a user
//! interacting with the launcher would see it.

use burst::app::App;
use burst::config::{Config, UiConfig};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

fn render(app: &mut App, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    app.render(area, &mut buf);
    buf
}

fn row_text(buf: &Buffer, y: u16) -> String {
    let area = *buf.area();
    (0..area.width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect()
}

fn frame_contains(buf: &Buffer, needle: &str) -> bool {
    let area = *buf.area();
    (0..area.height).any(|y| row_text(buf, y).contains(needle))
}

#[test]
fn app_starts_running_with_default_config() {
    let app = App::new(Config::default());
    assert!(app.running, "app should start in running state");
}

#[test]
fn escape_exits_full_app() {
    let mut app = App::new(Config::default());
    app.handle_key(KeyCode::Esc);
    assert!(!app.running, "Escape should stop the app");
}

#[test]
fn default_banner_renders_below_top_padding() {
    let mut app = App::new(Config::default());
    let buf = render(&mut app, 80, 24);
    let first_row = row_text(&buf, 1);
    assert!(
        first_row.trim_start().starts_with("_"),
        "expected banner art below top padding, got {:?}",
        first_row
    );
}

fn no_banner_config() -> Config {
    Config {
        ui: UiConfig {
            banner: String::new(),
            ..UiConfig::default()
        },
        ..Config::default()
    }
}

#[test]
fn custom_banner_overrides_default() {
    let cfg = Config {
        ui: UiConfig {
            banner: "CUSTOM-BANNER".to_string(),
            ..UiConfig::default()
        },
        ..Config::default()
    };
    let mut app = App::new(cfg);
    let buf = render(&mut app, 40, 10);
    assert!(
        frame_contains(&buf, "CUSTOM-BANNER"),
        "expected custom banner text in frame"
    );
}

#[test]
fn empty_banner_hides_banner_and_prompt_uses_top_padding() {
    let mut app = App::new(no_banner_config());
    let buf = render(&mut app, 40, 10);
    let first_row = row_text(&buf, 1);
    assert!(
        first_row.contains("> "),
        "with empty banner the prompt should render below top padding, got {:?}",
        first_row
    );
}

#[test]
fn typing_updates_visible_prompt() {
    let mut app = App::new(no_banner_config());
    for c in "firefox".chars() {
        app.handle_key(KeyCode::Char(c));
    }
    let buf = render(&mut app, 60, 10);
    assert!(
        frame_contains(&buf, "> firefox"),
        "prompt row should echo typed query"
    );
}

#[test]
fn backspace_reverts_prompt_characters() {
    let mut app = App::new(no_banner_config());
    for c in "abc".chars() {
        app.handle_key(KeyCode::Char(c));
    }
    app.handle_key(KeyCode::Backspace);
    app.handle_key(KeyCode::Backspace);
    let buf = render(&mut app, 40, 10);
    let prompt_row = row_text(&buf, 1);
    assert!(
        prompt_row.contains("> a") && !prompt_row.contains("> ab"),
        "expected single remaining char after two backspaces, got {:?}",
        prompt_row
    );
}

#[test]
fn apply_effects_is_safe_to_invoke_without_a_frame() {
    let mut app = App::new(Config::default());
    let _buf = render(&mut app, 80, 24);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _buf = render(&mut app, 80, 24);
}

#[test]
fn full_flow_type_navigate_escape() {
    let mut app = App::new(Config::default());
    for c in "xy".chars() {
        app.handle_key(KeyCode::Char(c));
    }
    app.handle_key(KeyCode::Down);
    app.handle_key(KeyCode::Up);
    app.handle_key(KeyCode::PageDown);
    app.handle_key(KeyCode::PageUp);
    assert!(app.running);
    app.handle_key(KeyCode::Esc);
    assert!(!app.running);
}

#[test]
fn events_are_ignored_after_app_stops() {
    let mut app = App::new(Config::default());
    app.handle_key(KeyCode::Esc);
    assert!(!app.running);
    app.handle_key(KeyCode::Char('a'));
    app.handle_key(KeyCode::Down);
    assert!(!app.running);
}

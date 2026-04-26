//! End-to-end integration tests that drive the full `App` through key events and
//! render it to a `Buffer`, asserting on the resulting frame just like a user
//! interacting with the launcher would see it.

use burst::app::{App, DiscoveryPayload};
use burst::config::{Config, UiConfig};
use burst::desktop::DesktopEntry;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;
use std::sync::mpsc;
use std::time::Duration;

fn app_entry(id: &str, name: &str) -> DesktopEntry {
    DesktopEntry {
        id: id.to_string(),
        name: name.to_string(),
        icon: String::new(),
        exec: id.to_string(),
    }
}

fn app_with_entries(config: Config, entries: Vec<DesktopEntry>) -> App {
    let (tx, rx) = mpsc::channel();
    tx.send(Ok(DiscoveryPayload::from_apps(entries))).unwrap();
    App::with_discovery_receiver(config, rx, Duration::from_millis(1))
}

fn app_with_default_entries(config: Config) -> App {
    app_with_entries(config, vec![app_entry("firefox", "Firefox")])
}

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
    let app = app_with_default_entries(Config::default());
    assert!(app.running, "app should start in running state");
}

#[test]
fn escape_exits_full_app() {
    let mut app = app_with_default_entries(Config::default());
    app.handle_key(KeyCode::Esc);
    assert!(!app.running, "Escape should stop the app");
}

#[test]
fn default_banner_renders_below_top_padding() {
    let mut app = app_with_default_entries(Config::default());
    let buf = render(&mut app, 80, 24);
    let first_row = row_text(&buf, 2);
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
fn pending_discovery_renders_loading_without_blocking_startup() {
    let (_tx, rx) = mpsc::channel();
    let mut app = App::with_discovery_receiver(no_banner_config(), rx, Duration::ZERO);

    let buf = render(&mut app, 60, 10);

    assert!(app.running);
    assert!(frame_contains(&buf, "Loading applications"));
}

#[test]
fn fast_discovery_skips_loading_state() {
    let (tx, rx) = mpsc::channel();
    tx.send(Ok(DiscoveryPayload::from_apps(vec![app_entry(
        "firefox", "Firefox",
    )])))
    .unwrap();

    let mut app = App::with_discovery_receiver(no_banner_config(), rx, Duration::from_millis(1));
    let buf = render(&mut app, 60, 10);

    assert!(frame_contains(&buf, "Firefox"));
    assert!(!frame_contains(&buf, "Loading applications"));
}

#[test]
fn query_typed_while_loading_is_applied_to_discovered_apps() {
    let (tx, rx) = mpsc::channel();
    let mut app = App::with_discovery_receiver(no_banner_config(), rx, Duration::ZERO);

    for c in "fir".chars() {
        app.handle_key(KeyCode::Char(c));
    }
    tx.send(Ok(DiscoveryPayload::from_apps(vec![
        app_entry("calculator", "Calculator"),
        app_entry("firefox", "Firefox"),
    ])))
    .unwrap();

    app.poll_discovery();
    let buf = render(&mut app, 60, 10);

    assert!(frame_contains(&buf, "> fir"));
    assert!(frame_contains(&buf, "Firefox"));
    assert!(!frame_contains(&buf, "Calculator"));
}

#[test]
fn escape_closes_app_while_discovery_is_pending() {
    let (_tx, rx) = mpsc::channel();
    let mut app = App::with_discovery_receiver(no_banner_config(), rx, Duration::ZERO);

    app.handle_key(KeyCode::Esc);

    assert!(!app.running);
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
    let mut app = app_with_default_entries(cfg);
    let buf = render(&mut app, 40, 10);
    assert!(
        frame_contains(&buf, "CUSTOM-BANNER"),
        "expected custom banner text in frame"
    );
}

#[test]
fn empty_banner_hides_banner_and_prompt_uses_top_padding() {
    let mut app = app_with_default_entries(no_banner_config());
    let buf = render(&mut app, 40, 10);
    let first_row = row_text(&buf, 2);
    assert!(
        first_row.contains("> "),
        "with empty banner the prompt should render below top padding, got {:?}",
        first_row
    );
}

#[test]
fn typing_updates_visible_prompt() {
    let mut app = app_with_default_entries(no_banner_config());
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
    let mut app = app_with_default_entries(no_banner_config());
    for c in "abc".chars() {
        app.handle_key(KeyCode::Char(c));
    }
    app.handle_key(KeyCode::Backspace);
    app.handle_key(KeyCode::Backspace);
    let buf = render(&mut app, 40, 10);
    let prompt_row = row_text(&buf, 2);
    assert!(
        prompt_row.contains("> a") && !prompt_row.contains("> ab"),
        "expected single remaining char after two backspaces, got {:?}",
        prompt_row
    );
}

#[test]
fn apply_effects_is_safe_to_invoke_without_a_frame() {
    let mut app = app_with_default_entries(Config::default());
    let _buf = render(&mut app, 80, 24);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _buf = render(&mut app, 80, 24);
}

#[test]
fn full_flow_type_navigate_escape() {
    let mut app = app_with_default_entries(Config::default());
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
    let mut app = app_with_default_entries(Config::default());
    app.handle_key(KeyCode::Esc);
    assert!(!app.running);
    app.handle_key(KeyCode::Char('a'));
    app.handle_key(KeyCode::Down);
    assert!(!app.running);
}

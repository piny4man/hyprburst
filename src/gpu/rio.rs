//! Rio's embeddable VT engine wired to a `hyprburst tui` child process.

use std::borrow::Cow;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use corcovado::channel;
use rio_vt::ansi::CursorShape;
use rio_vt::config::colors::term::TermColors;
use rio_vt::config::colors::{AnsiColor, NamedColor};
use rio_vt::crosswords::grid::Dimensions;
use rio_vt::crosswords::pos::Column;
use rio_vt::crosswords::square::ContentTag;
use rio_vt::crosswords::style::StyleFlags;
use rio_vt::crosswords::{Crosswords, Mode};
use rio_vt::event::sync::FairMutex;
use rio_vt::event::{EventListener, Msg, RioEvent, WindowId};
use rio_vt::performer::Machine;
use teletypewriter::{WinsizeBuilder, create_pty_with_spawn};

const ROUTE_ID: usize = 1;
const SCROLLBACK_LINES: usize = 1_000;

#[derive(Debug, PartialEq, Eq)]
struct ChildSpec {
    program: PathBuf,
    args: Vec<String>,
}

fn child_spec(executable: &Path) -> ChildSpec {
    ChildSpec {
        program: executable.to_path_buf(),
        args: vec!["tui".to_string()],
    }
}

#[derive(Clone)]
struct Listener {
    wake: Arc<dyn Fn() + Send + Sync>,
    closed: Arc<AtomicBool>,
    writer: Arc<Mutex<Option<channel::Sender<Msg>>>>,
}

impl Listener {
    fn dispatch(&self, event: RioEvent) {
        match event {
            RioEvent::TerminalDamaged(_) | RioEvent::Render | RioEvent::RenderRoute(_) => {
                (self.wake)();
            }
            RioEvent::PtyWrite(_, text) => {
                if let Some(writer) = self
                    .writer
                    .lock()
                    .expect("Rio PTY writer poisoned")
                    .as_ref()
                {
                    let _ = writer.send(Msg::Input(Cow::Owned(text.into_bytes())));
                }
            }
            RioEvent::CloseTerminal(_) | RioEvent::Exit => {
                self.closed.store(true, Ordering::Release);
                (self.wake)();
            }
            _ => {}
        }
    }
}

impl EventListener for Listener {
    fn event(&self) -> (Option<RioEvent>, bool) {
        (None, false)
    }

    fn send_event(&self, event: RioEvent, _window_id: WindowId) {
        self.dispatch(event);
    }

    fn send_event_with_high_priority(&self, event: RioEvent, _window_id: WindowId) {
        self.dispatch(event);
    }
}

#[derive(Clone, Copy)]
struct GridSize {
    cols: usize,
    rows: usize,
    cell_width: f32,
    cell_height: f32,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }

    fn square_width(&self) -> f32 {
        self.cell_width
    }

    fn square_height(&self) -> f32 {
        self.cell_height
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KeyInput {
    Text(String),
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
}

fn encode_key(key: KeyInput, app_cursor: bool) -> Vec<u8> {
    match key {
        KeyInput::Text(text) => text.into_bytes(),
        KeyInput::Enter => vec![b'\r'],
        KeyInput::Tab => vec![b'\t'],
        KeyInput::Backspace => vec![0x7f],
        KeyInput::Escape => vec![0x1b],
        KeyInput::Up => cursor_sequence(b'A', app_cursor),
        KeyInput::Down => cursor_sequence(b'B', app_cursor),
        KeyInput::Right => cursor_sequence(b'C', app_cursor),
        KeyInput::Left => cursor_sequence(b'D', app_cursor),
        KeyInput::PageUp => b"\x1b[5~".to_vec(),
        KeyInput::PageDown => b"\x1b[6~".to_vec(),
    }
}

fn cursor_sequence(final_byte: u8, app_cursor: bool) -> Vec<u8> {
    vec![0x1b, if app_cursor { b'O' } else { b'[' }, final_byte]
}

fn encode_mouse_press(col: u16, row: u16) -> Vec<u8> {
    format!("\x1b[<0;{};{}M", col as u32 + 1, row as u32 + 1).into_bytes()
}

pub(crate) struct TerminalGlyph {
    pub col: u16,
    pub row: u16,
    pub ch: char,
    pub bold: bool,
    pub color: [f32; 3],
}

pub(crate) struct TerminalBackground {
    pub col: u16,
    pub row: u16,
    pub color: [f32; 4],
}

pub(crate) struct TerminalFrame {
    pub backgrounds: Vec<TerminalBackground>,
    pub glyphs: Vec<TerminalGlyph>,
}

pub(crate) struct Session {
    terminal: Arc<FairMutex<Crosswords<Listener>>>,
    channel: channel::Sender<Msg>,
    closed: Arc<AtomicBool>,
    child_pid: i32,
}

impl Session {
    pub fn spawn(
        executable: &Path,
        grid: (u16, u16),
        pixels: (u32, u32),
        cell: (u32, u32),
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let spec = child_spec(executable);
        let closed = Arc::new(AtomicBool::new(false));
        let writer = Arc::new(Mutex::new(None));
        let listener = Listener {
            wake,
            closed: Arc::clone(&closed),
            writer: Arc::clone(&writer),
        };
        let dimensions = GridSize {
            cols: grid.0.max(1) as usize,
            rows: grid.1.max(1) as usize,
            cell_width: cell.0.max(1) as f32,
            cell_height: cell.1.max(1) as f32,
        };
        let terminal = Arc::new(FairMutex::new(Crosswords::new(
            dimensions,
            CursorShape::Block,
            listener.clone(),
            WindowId::from(ROUTE_ID as u64),
            ROUTE_ID,
            SCROLLBACK_LINES,
        )));
        let program = spec.program.to_string_lossy();
        let working_directory = std::env::current_dir()
            .ok()
            .map(|path| path.to_string_lossy().into_owned());
        let pty = create_pty_with_spawn(
            &Cow::Borrowed(program.as_ref()),
            spec.args,
            &working_directory,
            grid.0.max(1),
            grid.1.max(1),
            clamp_u16(pixels.0),
            clamp_u16(pixels.1),
        )?;
        let child_pid = *pty.child.pid.clone() as i32;
        let machine = Machine::new(
            Arc::clone(&terminal),
            pty,
            listener,
            WindowId::from(ROUTE_ID as u64),
            ROUTE_ID,
        )
        .map_err(|error| std::io::Error::other(error.to_string()))?;
        let channel = machine.channel();
        *writer.lock().expect("Rio PTY writer poisoned") = Some(channel.clone());
        drop(machine.spawn());

        Ok(Self {
            terminal,
            channel,
            closed,
            child_pid,
        })
    }

    pub fn send_key(&self, key: KeyInput) {
        let app_cursor = self.terminal.lock().mode().contains(Mode::APP_CURSOR);
        let _ = self
            .channel
            .send(Msg::Input(Cow::Owned(encode_key(key, app_cursor))));
    }

    pub fn send_mouse_press(&self, col: u16, row: u16) {
        let mode = self.terminal.lock().mode();
        if !mode.intersects(Mode::MOUSE_MODE) || !mode.contains(Mode::SGR_MOUSE) {
            return;
        }
        let _ = self
            .channel
            .send(Msg::Input(Cow::Owned(encode_mouse_press(col, row))));
    }

    pub fn resize(&self, grid: (u16, u16), pixels: (u32, u32), cell: (u32, u32)) {
        self.terminal.lock().resize(GridSize {
            cols: grid.0.max(1) as usize,
            rows: grid.1.max(1) as usize,
            cell_width: cell.0.max(1) as f32,
            cell_height: cell.1.max(1) as f32,
        });
        let _ = self.channel.send(Msg::Resize(WinsizeBuilder {
            rows: grid.1.max(1),
            cols: grid.0.max(1),
            width: clamp_u16(pixels.0),
            height: clamp_u16(pixels.1),
        }));
    }

    pub fn frame(&self, default_fg: [f32; 3], default_bg: [f32; 3]) -> TerminalFrame {
        consume_frame(&mut self.terminal.lock(), default_fg, default_bg)
    }

    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}

fn consume_frame<U: EventListener>(
    terminal: &mut Crosswords<U>,
    default_fg: [f32; 3],
    default_bg: [f32; 3],
) -> TerminalFrame {
    let frame = snapshot(terminal, default_fg, default_bg);
    terminal.reset_damage();
    terminal.damage_event_in_flight = false;
    frame
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.channel.send(Msg::Shutdown);
        teletypewriter::kill_pid(self.child_pid);
    }
}

fn snapshot<U: EventListener>(
    terminal: &Crosswords<U>,
    default_fg: [f32; 3],
    default_bg: [f32; 3],
) -> TerminalFrame {
    let rows = terminal.visible_rows();
    let mut backgrounds = Vec::new();
    let mut glyphs = Vec::new();

    for (row, cells) in rows.iter().enumerate() {
        for col in 0..terminal.columns() {
            let square = cells[Column(col)];
            if square.content_tag() != ContentTag::Codepoint {
                let rgb = match square.content_tag() {
                    ContentTag::BgPalette => indexed_rgb(square.bg_palette_index()),
                    ContentTag::BgRgb => rgb_norm(square.bg_rgb()),
                    ContentTag::Codepoint => unreachable!(),
                };
                backgrounds.push(TerminalBackground {
                    col: col as u16,
                    row: row as u16,
                    color: [rgb[0], rgb[1], rgb[2], 1.0],
                });
                continue;
            }

            let style = terminal.grid.style_set.get(square.style_id());
            let inverse = style.flags.contains(StyleFlags::INVERSE);
            let (fg, bg) = if inverse {
                (style.bg, style.fg)
            } else {
                (style.fg, style.bg)
            };
            let fg = ansi_rgb(fg, terminal.colors(), default_fg, default_bg);
            let bg_is_default =
                !inverse && matches!(style.bg, AnsiColor::Named(NamedColor::Background));
            if !bg_is_default {
                let bg = ansi_rgb(bg, terminal.colors(), default_fg, default_bg);
                backgrounds.push(TerminalBackground {
                    col: col as u16,
                    row: row as u16,
                    color: [bg[0], bg[1], bg[2], 1.0],
                });
            }

            let ch = square.c();
            if ch != '\0' && ch != ' ' && !style.flags.contains(StyleFlags::HIDDEN) {
                glyphs.push(TerminalGlyph {
                    col: col as u16,
                    row: row as u16,
                    ch,
                    bold: style.flags.contains(StyleFlags::BOLD),
                    color: fg,
                });
            }
        }
    }

    TerminalFrame {
        backgrounds,
        glyphs,
    }
}

fn ansi_rgb(
    color: AnsiColor,
    overrides: &TermColors,
    default_fg: [f32; 3],
    default_bg: [f32; 3],
) -> [f32; 3] {
    match color {
        AnsiColor::Spec(rgb) => rgb_norm((rgb.r, rgb.g, rgb.b)),
        AnsiColor::Indexed(index) => overrides[index as usize]
            .map(array_rgb)
            .unwrap_or_else(|| indexed_rgb(index)),
        AnsiColor::Named(named) => overrides[named]
            .map(array_rgb)
            .unwrap_or_else(|| named_rgb(named, default_fg, default_bg)),
    }
}

fn named_rgb(named: NamedColor, default_fg: [f32; 3], default_bg: [f32; 3]) -> [f32; 3] {
    match named {
        NamedColor::Foreground => default_fg,
        NamedColor::Background => default_bg,
        NamedColor::Cursor | NamedColor::LightForeground => default_fg,
        NamedColor::DimForeground => dim(default_fg),
        NamedColor::Black => indexed_rgb(0),
        NamedColor::Red => indexed_rgb(1),
        NamedColor::Green => indexed_rgb(2),
        NamedColor::Yellow => indexed_rgb(3),
        NamedColor::Blue => indexed_rgb(4),
        NamedColor::Magenta => indexed_rgb(5),
        NamedColor::Cyan => indexed_rgb(6),
        NamedColor::White => indexed_rgb(7),
        NamedColor::LightBlack => indexed_rgb(8),
        NamedColor::LightRed => indexed_rgb(9),
        NamedColor::LightGreen => indexed_rgb(10),
        NamedColor::LightYellow => indexed_rgb(11),
        NamedColor::LightBlue => indexed_rgb(12),
        NamedColor::LightMagenta => indexed_rgb(13),
        NamedColor::LightCyan => indexed_rgb(14),
        NamedColor::LightWhite => indexed_rgb(15),
        NamedColor::DimBlack => dim(indexed_rgb(0)),
        NamedColor::DimRed => dim(indexed_rgb(1)),
        NamedColor::DimGreen => dim(indexed_rgb(2)),
        NamedColor::DimYellow => dim(indexed_rgb(3)),
        NamedColor::DimBlue => dim(indexed_rgb(4)),
        NamedColor::DimMagenta => dim(indexed_rgb(5)),
        NamedColor::DimCyan => dim(indexed_rgb(6)),
        NamedColor::DimWhite => dim(indexed_rgb(7)),
    }
}

fn indexed_rgb(index: u8) -> [f32; 3] {
    let rgb = match index {
        0..=15 => {
            const BASE: [(u8, u8, u8); 16] = [
                (0, 0, 0),
                (205, 0, 0),
                (0, 205, 0),
                (205, 205, 0),
                (0, 0, 238),
                (205, 0, 205),
                (0, 205, 205),
                (229, 229, 229),
                (127, 127, 127),
                (255, 0, 0),
                (0, 255, 0),
                (255, 255, 0),
                (92, 92, 255),
                (255, 0, 255),
                (0, 255, 255),
                (255, 255, 255),
            ];
            BASE[index as usize]
        }
        16..=231 => {
            let index = index - 16;
            let steps = [0, 95, 135, 175, 215, 255];
            (
                steps[(index / 36) as usize],
                steps[((index / 6) % 6) as usize],
                steps[(index % 6) as usize],
            )
        }
        _ => {
            let value = 8 + 10 * (index - 232);
            (value, value, value)
        }
    };
    rgb_norm(rgb)
}

fn clamp_u16(value: u32) -> u16 {
    value.clamp(1, u16::MAX as u32) as u16
}

fn rgb_norm((r, g, b): (u8, u8, u8)) -> [f32; 3] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

fn array_rgb(color: [f32; 4]) -> [f32; 3] {
    [color[0], color[1], color[2]]
}

fn dim(color: [f32; 3]) -> [f32; 3] {
    [color[0] * 0.66, color[1] * 0.66, color[2] * 0.66]
}

#[cfg(test)]
mod tests {
    use rio_vt::event::VoidListener;
    use rio_vt::performer::handler::Processor;

    use super::*;

    #[test]
    fn child_runs_the_inline_tui() {
        let spec = child_spec(Path::new("/tmp/hypr burst"));

        assert_eq!(spec.program, Path::new("/tmp/hypr burst"));
        assert_eq!(spec.args, ["tui"]);
    }

    #[test]
    fn rio_vt_parses_the_tui_grid_and_resizes() {
        let mut terminal = Crosswords::new(
            GridSize {
                cols: 20,
                rows: 4,
                cell_width: 8.0,
                cell_height: 16.0,
            },
            CursorShape::Block,
            VoidListener,
            WindowId::from(0),
            0,
            0,
        );
        let mut parser = Processor::default();
        parser.advance(&mut terminal, b"\x1b[31mhyprburst\x1b[0m");
        terminal.damage_event_in_flight = true;

        let frame = consume_frame(&mut terminal, [1.0; 3], [0.0; 3]);
        let text: String = frame.glyphs.iter().map(|cell| cell.ch).collect();
        assert_eq!(text, "hyprburst");
        assert_eq!(frame.glyphs[0].color, indexed_rgb(1));
        assert!(!terminal.damage_event_in_flight);

        terminal.resize(GridSize {
            cols: 10,
            rows: 2,
            cell_width: 8.0,
            cell_height: 16.0,
        });
        assert_eq!(terminal.columns(), 10);
        assert_eq!(terminal.screen_lines(), 2);
    }

    #[test]
    fn keyboard_input_uses_terminal_sequences() {
        assert_eq!(
            encode_key(KeyInput::Text("ñ".into()), false),
            "ñ".as_bytes()
        );
        assert_eq!(encode_key(KeyInput::Enter, false), b"\r");
        assert_eq!(encode_key(KeyInput::Up, false), b"\x1b[A");
        assert_eq!(encode_key(KeyInput::Up, true), b"\x1bOA");
        assert_eq!(encode_key(KeyInput::PageDown, false), b"\x1b[6~");
    }

    #[test]
    fn mouse_press_uses_one_based_sgr_coordinates() {
        assert_eq!(encode_mouse_press(0, 0), b"\x1b[<0;1;1M");
        assert_eq!(encode_mouse_press(12, 4), b"\x1b[<0;13;5M");
    }
}

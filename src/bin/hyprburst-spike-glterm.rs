//! Phase 7 deliverable 2: the slim gl-term host for the Freya bake-off.
//!
//! A resident terminal host built *without* Freya/Skia — a winit window, a
//! hand-written OpenGL cell renderer (glutin context + glow draw + an `ab_glyph`
//! glyph atlas), and an `alacritty_terminal` PTY running the unmodified
//! `hyprburst tui`. It answers gate 5: can you own the surface and feel instant
//! *without* Skia's 278-crate tree? Its visual ceiling is glyphs — a terminal
//! grid can't draw raster themed icons — which is the structural trade vs. the
//! native-GUI variant.
//!
//! Built only with the `glterm-spike` feature (see `required-features` in
//! `Cargo.toml`); `freya-spike` turns that on, and building `glterm-spike` alone
//! pulls this slim stack *without* Skia — the build that yields the gate-5 dep
//! count.
//!
//! Modes (mirroring the other spike binaries):
//! - default — open the window and host `hyprburst tui` interactively.
//! - `--measure` — exit right after the first frame is presented (cold-start /
//!   peak-RSS capture).
//! - `--resident` — stay alive after the inner TUI launches an app: hide the
//!   window (Hyprland special workspace) and re-spawn a clean inner TUI, so the
//!   window can be toggled back on *warm*. `SIGUSR1` arms a warm-toggle
//!   measurement (show-trigger → first present), reported to stderr.
//! - `--bench` — no window: print the baseline-vs-gl-term comparison table.
//!
//! The renderer drives the *same* tested layout/atlas/diff core as the harness
//! model (see [`hyprburst::glterm`]); only the GL upload/draw and PTY plumbing
//! live here, and those are verified in a live Hyprland session.

use std::num::NonZeroU32;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ab_glyph::{Font, FontVec, ScaleFont};
use alacritty_terminal::event::{Event as TermEvent, EventListener, Notify, WindowSize};
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty::{self, Options as PtyOptions, Shell};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
use glow::HasContext;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextAttributesBuilder, PossiblyCurrentContext};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::{Window, WindowAttributes, WindowId};

use hyprburst::bench;
use hyprburst::glterm::{Atlas, CellMetrics, GlyphKey, cell_rect, grid_size};
use hyprburst::spike_metrics::WarmToggle;
use hyprburst::term_host::{INNER_ARGS, inner_binary_path};

/// Wayland app-id — must match the id the shipped windowrules target.
const APP_ID: &str = "hyprburst";
/// Column label this variant fills in the comparison table.
const VARIANT: &str = "gl-term";
/// Initial window size, matching the other spike binaries.
const WINDOW_W: u32 = 640;
const WINDOW_H: u32 = 720;
/// Glyph cell pixel size (a plausible monospace cell at ~16px); the live grid
/// derives from this and the window size.
const CELL_W: u32 = 9;
const CELL_H: u32 = 18;
/// Atlas texture dimensions — ample for a launcher's character set.
const ATLAS_W: u32 = 1024;
const ATLAS_H: u32 = 1024;

/// Process-start reference, set once at the top of `main`.
static START: OnceLock<Instant> = OnceLock::new();
/// Nanoseconds from process start to the first frame actually *presented* (buffer
/// swapped). The honest cold-start: time-to-visible. `0` = not yet painted.
static FIRST_PRESENT_NS: AtomicU64 = AtomicU64::new(0);
/// Warm-toggle capture (show-trigger → first present after re-show).
static WARM: WarmToggle = WarmToggle::new();
/// Whether the process runs resident (`--resident`).
static RESIDENT: AtomicBool = AtomicBool::new(false);
/// Exit the process right after the first present (`--measure`).
static MEASURE: AtomicBool = AtomicBool::new(false);

fn main() {
    let start = Instant::now();
    let _ = START.set(start);
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Headless benchmark mode: no window, baseline vs gl-term table.
    if args.iter().any(|a| a == "--bench") {
        print!("{}", bench::run_gl_term_report());
        return;
    }

    MEASURE.store(args.iter().any(|a| a == "--measure"), Ordering::SeqCst);
    let resident = args.iter().any(|a| a == "--resident");
    RESIDENT.store(resident, Ordering::SeqCst);

    let font = match load_monospace_font() {
        Some(font) => font,
        None => {
            eprintln!(
                "hyprburst-spike-glterm: no monospace font found; set HYPRBURST_GLTERM_FONT to a .ttf/.otf path"
            );
            std::process::exit(1);
        }
    };

    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(err) => {
            eprintln!(
                "hyprburst-spike-glterm: cannot create event loop ({err}) — is a Wayland display available?"
            );
            std::process::exit(1);
        }
    };
    let proxy = event_loop.create_proxy();

    if resident {
        spawn_warm_reporter();
        spawn_show_trigger_listener(proxy.clone());
    }

    let mut app = App::new(start, font, proxy);
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("hyprburst-spike-glterm: event loop exited with error: {err}");
        std::process::exit(1);
    }
}

/// User events posted from the PTY's `EventListener` (off-thread) onto the winit
/// loop, so all window/GL work stays on the main thread.
#[derive(Debug, Clone)]
enum UserEvent {
    /// New terminal content available — request a redraw.
    Wakeup,
    /// The terminal asked to write a response back to the PTY.
    PtyWrite(String),
    /// The inner child process exited.
    ChildExit,
}

/// Bridges `alacritty_terminal`'s off-thread events onto the winit event loop.
/// Cloneable + `Send`, as the PTY event loop requires of its listener.
#[derive(Clone)]
struct EventProxy(EventLoopProxy<UserEvent>);

impl EventListener for EventProxy {
    fn send_event(&self, event: TermEvent) {
        match event {
            TermEvent::Wakeup => {
                let _ = self.0.send_event(UserEvent::Wakeup);
            }
            TermEvent::PtyWrite(text) => {
                let _ = self.0.send_event(UserEvent::PtyWrite(text));
            }
            TermEvent::ChildExit(_) => {
                let _ = self.0.send_event(UserEvent::ChildExit);
            }
            _ => {}
        }
    }
}

/// Terminal grid dimensions for `Term`/`tty`, derived from the window + cell size.
#[derive(Clone, Copy)]
struct TermDims {
    cols: usize,
    rows: usize,
}

impl Dimensions for TermDims {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// A running inner TUI: the shared terminal grid plus the channel that writes
/// keystrokes to its PTY.
struct Inner {
    term: Arc<FairMutex<Term<EventProxy>>>,
    notifier: Notifier,
}

/// Spawn `hyprburst tui` in a PTY of `dims`, with an off-thread event loop that
/// parses its output into a shared `Term`. Returns the grid handle + an input
/// channel. The `JoinHandle` is detached (the loop ends on child exit / shutdown).
fn spawn_inner(proxy: &EventLoopProxy<UserEvent>, dims: TermDims) -> std::io::Result<Inner> {
    tty::setup_env();

    let program = inner_binary_path(std::env::current_exe().ok());
    let shell = Shell::new(
        program.to_string_lossy().into_owned(),
        INNER_ARGS.iter().map(|a| a.to_string()).collect(),
    );
    let mut options = PtyOptions {
        shell: Some(shell),
        ..Default::default()
    };
    options.env.insert("TERM".into(), "xterm-256color".into());

    let window_size = WindowSize {
        num_cols: dims.cols as u16,
        num_lines: dims.rows as u16,
        cell_width: CELL_W as u16,
        cell_height: CELL_H as u16,
    };

    let pty = tty::new(&options, window_size, 0)?;
    let event_proxy = EventProxy(proxy.clone());
    let term = Arc::new(FairMutex::new(Term::new(
        TermConfig::default(),
        &dims,
        event_proxy.clone(),
    )));

    let pty_loop = PtyEventLoop::new(term.clone(), event_proxy, pty, false, false)?;
    let notifier = Notifier(pty_loop.channel());
    let _ = pty_loop.spawn();

    Ok(Inner { term, notifier })
}

/// The winit application: holds GL state once resumed, the inner TUI, and the
/// renderer. All window/GL mutation happens on the main thread via the handler.
struct App {
    start: Instant,
    font: FontVec,
    proxy: EventLoopProxy<UserEvent>,
    gl: Option<GlState>,
    inner: Option<Inner>,
    dims: TermDims,
}

impl App {
    fn new(start: Instant, font: FontVec, proxy: EventLoopProxy<UserEvent>) -> Self {
        let cols = (WINDOW_W / CELL_W).max(1) as usize;
        let rows = (WINDOW_H / CELL_H).max(1) as usize;
        Self {
            start,
            font,
            proxy,
            gl: None,
            inner: None,
            dims: TermDims { cols, rows },
        }
    }

    /// Hide the resident window via the Hyprland special workspace and re-spawn a
    /// clean inner TUI, so the next toggle shows a warm launcher.
    fn hide_and_respawn(&mut self) {
        hyprburst::hyprland::dispatch_toggle_special(APP_ID);
        match spawn_inner(&self.proxy, self.dims) {
            Ok(inner) => self.inner = Some(inner),
            Err(err) => eprintln!("hyprburst-spike-glterm: could not re-spawn inner TUI: {err}"),
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gl.is_some() {
            return;
        }

        let window_attributes = WindowAttributes::default()
            .with_title("hyprburst (gl-term spike)")
            // On Wayland the general name becomes the app-id the windowrules match.
            .with_name(APP_ID, APP_ID)
            .with_inner_size(winit::dpi::LogicalSize::new(WINDOW_W, WINDOW_H))
            .with_transparent(true);

        let gl = match GlState::new(event_loop, window_attributes, &self.font) {
            Ok(gl) => gl,
            Err(err) => {
                eprintln!("hyprburst-spike-glterm: GL bootstrap failed: {err}");
                event_loop.exit();
                return;
            }
        };

        // Size the grid to the realized window before spawning the PTY.
        let size = gl.window.inner_size();
        let grid = grid_size((size.width, size.height), CellMetrics::new(CELL_W, CELL_H));
        self.dims = TermDims {
            cols: grid.cols as usize,
            rows: grid.rows as usize,
        };

        match spawn_inner(&self.proxy, self.dims) {
            Ok(inner) => self.inner = Some(inner),
            Err(err) => {
                eprintln!("hyprburst-spike-glterm: could not spawn inner TUI: {err}");
                event_loop.exit();
                return;
            }
        }

        gl.window.request_redraw();
        self.gl = Some(gl);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Wakeup => {
                if let Some(gl) = &self.gl {
                    gl.window.request_redraw();
                }
            }
            UserEvent::PtyWrite(text) => {
                if let Some(inner) = &self.inner {
                    inner.notifier.notify(text.into_bytes());
                }
            }
            UserEvent::ChildExit => {
                if RESIDENT.load(Ordering::SeqCst) {
                    // Stay alive: hide and re-arm a fresh launcher for the next show.
                    self.hide_and_respawn();
                    if let Some(gl) = &self.gl {
                        gl.window.request_redraw();
                    }
                } else {
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gl) = &mut self.gl {
                    gl.resize(size.width, size.height);
                }
                let grid = grid_size((size.width, size.height), CellMetrics::new(CELL_W, CELL_H));
                self.dims = TermDims {
                    cols: grid.cols as usize,
                    rows: grid.rows as usize,
                };
                if let Some(inner) = &self.inner {
                    let _ = inner.notifier.0.send(Msg::Resize(WindowSize {
                        num_cols: grid.cols,
                        num_lines: grid.rows,
                        cell_width: CELL_W as u16,
                        cell_height: CELL_H as u16,
                    }));
                    inner.term.lock().resize(self.dims);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && let Some(bytes) = key_to_bytes(&event)
                    && let Some(inner) = &self.inner
                {
                    inner.notifier.notify(bytes);
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(gl), Some(inner)) = (&mut self.gl, &self.inner) {
                    gl.render(&self.font, &inner.term);
                    // Stamp the honest cold-start at the first present.
                    let ns = (self.start.elapsed().as_nanos() as u64).max(1);
                    let first = FIRST_PRESENT_NS
                        .compare_exchange(0, ns, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok();
                    // Stamp warm-toggle (no-op unless a show-trigger was armed).
                    let _ = WARM.present(ns);
                    if first && MEASURE.load(Ordering::SeqCst) {
                        report(ns);
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Map a winit key press to the bytes the PTY expects, covering the keys the
/// launcher TUI uses (text, Enter, Backspace, Esc, Tab, arrows).
fn key_to_bytes(event: &KeyEvent) -> Option<Vec<u8>> {
    match &event.logical_key {
        Key::Named(NamedKey::Enter) => Some(vec![b'\r']),
        Key::Named(NamedKey::Backspace) => Some(vec![0x7f]),
        Key::Named(NamedKey::Escape) => Some(vec![0x1b]),
        Key::Named(NamedKey::Tab) => Some(vec![b'\t']),
        Key::Named(NamedKey::ArrowUp) => Some(b"\x1b[A".to_vec()),
        Key::Named(NamedKey::ArrowDown) => Some(b"\x1b[B".to_vec()),
        Key::Named(NamedKey::ArrowRight) => Some(b"\x1b[C".to_vec()),
        Key::Named(NamedKey::ArrowLeft) => Some(b"\x1b[D".to_vec()),
        Key::Named(NamedKey::Space) => Some(vec![b' ']),
        _ => event
            .text
            .as_ref()
            .filter(|t| !t.is_empty())
            .map(|t| t.as_bytes().to_vec()),
    }
}

/// GL state realized after the window is created: the surface, the current
/// context, the glow context, and the cell renderer.
struct GlState {
    window: Window,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    gl: glow::Context,
    renderer: CellRenderer,
}

impl GlState {
    fn new(
        event_loop: &ActiveEventLoop,
        window_attributes: WindowAttributes,
        font: &FontVec,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_transparency(true);
        let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attributes));

        let (window, gl_config) = display_builder.build(event_loop, template, |configs| {
            // Prefer a config with the most alpha bits (for the blur windowrule).
            configs
                .reduce(|acc, cfg| {
                    if cfg.alpha_size() > acc.alpha_size() {
                        cfg
                    } else {
                        acc
                    }
                })
                .expect("no GL config")
        })?;
        let window = window.ok_or("winit did not create a window")?;

        let raw_window_handle = window.window_handle()?.as_raw();
        let gl_display = gl_config.display();

        let context_attributes = ContextAttributesBuilder::new().build(Some(raw_window_handle));
        let not_current = unsafe { gl_display.create_context(&gl_config, &context_attributes)? };

        let surface_attrs =
            window.build_surface_attributes(SurfaceAttributesBuilder::<WindowSurface>::new())?;
        let surface = unsafe { gl_display.create_window_surface(&gl_config, &surface_attrs)? };
        let context = not_current.make_current(&surface)?;

        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|s| gl_display.get_proc_address(s).cast())
        };

        let renderer = unsafe { CellRenderer::new(&gl, font) };

        Ok(Self {
            window,
            surface,
            context,
            gl,
            renderer,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) {
            self.surface.resize(&self.context, w, h);
            unsafe { self.gl.viewport(0, 0, width as i32, height as i32) };
        }
    }

    /// Snapshot the terminal grid and draw it, then present.
    fn render(&mut self, font: &FontVec, term: &Arc<FairMutex<Term<EventProxy>>>) {
        let size = self.window.inner_size();
        let cells = {
            let term = term.lock();
            let mut cells = Vec::new();
            for indexed in term.grid().display_iter() {
                let ch = indexed.c;
                if ch == ' ' || ch == '\0' {
                    continue;
                }
                cells.push(CellInstance {
                    col: indexed.point.column.0 as u16,
                    row: indexed.point.line.0.max(0) as u16,
                    ch,
                    bold: indexed.flags.contains(Flags::BOLD),
                    color: ansi_rgb(indexed.fg),
                });
            }
            cells
        };
        unsafe {
            self.renderer
                .draw(&self.gl, font, size.width, size.height, &cells);
        }
        let _ = self.surface.swap_buffers(&self.context);
    }
}

/// One non-blank terminal cell to draw, resolved from the emulator grid.
struct CellInstance {
    col: u16,
    row: u16,
    ch: char,
    bold: bool,
    color: [f32; 3],
}

/// The OpenGL cell renderer: a textured-quad shader, a glyph atlas texture fed by
/// `ab_glyph` rasterization, and the tested [`Atlas`] keying which glyph lives
/// where. Each frame builds a vertex buffer of one quad per non-blank cell and
/// draws it in a single call.
struct CellRenderer {
    program: glow::Program,
    vao: glow::VertexArray,
    vbo: glow::Buffer,
    viewport_loc: Option<glow::UniformLocation>,
    atlas_loc: Option<glow::UniformLocation>,
    atlas_tex: glow::Texture,
    atlas: Atlas,
    scale: ab_glyph::PxScale,
    ascent: f32,
}

/// Append one vertex (pos.xy px, uv.xy, color.rgb) to the buffer.
fn push_vert(buf: &mut Vec<f32>, x: f32, y: f32, u: f32, v: f32, color: [f32; 3]) {
    buf.extend_from_slice(&[x, y, u, v, color[0], color[1], color[2]]);
}

/// Floats per vertex: pos.xy (px), uv.xy, color.rgb.
const VERTEX_FLOATS: usize = 7;

impl CellRenderer {
    /// # Safety
    /// `gl` must be a current context for the lifetime of the renderer.
    unsafe fn new(gl: &glow::Context, font: &FontVec) -> Self {
        let program = unsafe { build_program(gl) };
        let vao = unsafe { gl.create_vertex_array().expect("vao") };
        let vbo = unsafe { gl.create_buffer().expect("vbo") };
        let viewport_loc = unsafe { gl.get_uniform_location(program, "u_viewport") };
        let atlas_loc = unsafe { gl.get_uniform_location(program, "u_atlas") };

        let atlas_tex = unsafe { gl.create_texture().expect("atlas tex") };
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(atlas_tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R8 as i32,
                ATLAS_W as i32,
                ATLAS_H as i32,
                0,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&vec![0u8; (ATLAS_W * ATLAS_H) as usize])),
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        }

        let scale = ab_glyph::PxScale::from(CELL_H as f32 * 0.85);
        let ascent = font.as_scaled(scale).ascent();

        Self {
            program,
            vao,
            vbo,
            viewport_loc,
            atlas_loc,
            atlas_tex,
            atlas: Atlas::new((ATLAS_W, ATLAS_H), CellMetrics::new(CELL_W, CELL_H)),
            scale,
            ascent,
        }
    }

    /// Ensure `(ch, bold)` is resident in the atlas, rasterizing + uploading it on
    /// first sight. Returns its normalized UV rect, or `None` if the atlas is full.
    fn ensure_glyph(
        &mut self,
        gl: &glow::Context,
        font: &FontVec,
        ch: char,
        bold: bool,
    ) -> Option<(f32, f32, f32, f32)> {
        let slot = self.atlas.get_or_insert(GlyphKey::new(ch, bold))?;
        if slot.newly_inserted {
            self.rasterize_into(gl, font, ch, slot.px);
        }
        Some(self.atlas.uv_rect(slot))
    }

    /// Rasterize `ch` into the atlas tile at pixel origin `px` via `ab_glyph`.
    fn rasterize_into(&self, gl: &glow::Context, font: &FontVec, ch: char, px: (u32, u32)) {
        let mut coverage = vec![0u8; (CELL_W * CELL_H) as usize];
        let glyph = font
            .glyph_id(ch)
            .with_scale_and_position(self.scale, ab_glyph::point(0.0, self.ascent));
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, c| {
                let x = gx as i32 + bounds.min.x as i32;
                let y = gy as i32 + bounds.min.y as i32;
                if x >= 0 && (x as u32) < CELL_W && y >= 0 && (y as u32) < CELL_H {
                    let idx = (y as u32 * CELL_W + x as u32) as usize;
                    coverage[idx] = (c * 255.0) as u8;
                }
            });
        }
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas_tex));
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                px.0 as i32,
                px.1 as i32,
                CELL_W as i32,
                CELL_H as i32,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&coverage)),
            );
        }
    }

    /// Draw all visible cells in a single call: ensure each glyph is atlas-
    /// resident (rasterizing + uploading on a miss), build one interleaved vertex
    /// buffer of quads, upload it, and `glDrawArrays`. This is the only place with
    /// the live GL context, so glyph upload happens here.
    ///
    /// # Safety
    /// `gl` must be the current context.
    unsafe fn draw(
        &mut self,
        gl: &glow::Context,
        font: &FontVec,
        width: u32,
        height: u32,
        cells: &[CellInstance],
    ) {
        let cell = CellMetrics::new(CELL_W, CELL_H);
        let mut verts: Vec<f32> = Vec::with_capacity(cells.len() * 6 * VERTEX_FLOATS);
        for c in cells {
            let Some((u0, v0, u1, v1)) = self.ensure_glyph(gl, font, c.ch, c.bold) else {
                continue;
            };
            let (x, y, w, h) = cell_rect(c.col, c.row, cell);
            let (x0, y0, x1, y1) = (x as f32, y as f32, (x + w) as f32, (y + h) as f32);
            let col = c.color;
            push_vert(&mut verts, x0, y0, u0, v0, col);
            push_vert(&mut verts, x1, y0, u1, v0, col);
            push_vert(&mut verts, x1, y1, u1, v1, col);
            push_vert(&mut verts, x0, y0, u0, v0, col);
            push_vert(&mut verts, x1, y1, u1, v1, col);
            push_vert(&mut verts, x0, y1, u0, v1, col);
        }

        unsafe {
            gl.viewport(0, 0, width as i32, height as i32);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            if verts.is_empty() {
                return;
            }

            gl.use_program(Some(self.program));
            gl.uniform_2_f32(self.viewport_loc.as_ref(), width as f32, height as f32);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas_tex));
            gl.uniform_1_i32(self.atlas_loc.as_ref(), 0);

            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            let bytes = core::slice::from_raw_parts(
                verts.as_ptr() as *const u8,
                std::mem::size_of_val(verts.as_slice()),
            );
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::DYNAMIC_DRAW);

            let stride = (VERTEX_FLOATS * std::mem::size_of::<f32>()) as i32;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, stride, 2 * 4);
            gl.enable_vertex_attrib_array(2);
            gl.vertex_attrib_pointer_f32(2, 3, glow::FLOAT, false, stride, 4 * 4);

            let count = (verts.len() / VERTEX_FLOATS) as i32;
            gl.draw_arrays(glow::TRIANGLES, 0, count);
        }
    }
}

/// Compile the textured-quad shader program.
///
/// # Safety
/// `gl` must be a current context.
unsafe fn build_program(gl: &glow::Context) -> glow::Program {
    const VS: &str = r#"#version 330 core
layout (location = 0) in vec2 a_pos;
layout (location = 1) in vec2 a_uv;
layout (location = 2) in vec3 a_color;
uniform vec2 u_viewport;
out vec2 v_uv;
out vec3 v_color;
void main() {
    vec2 ndc = vec2(a_pos.x / u_viewport.x * 2.0 - 1.0, 1.0 - a_pos.y / u_viewport.y * 2.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_uv = a_uv;
    v_color = a_color;
}
"#;
    const FS: &str = r#"#version 330 core
in vec2 v_uv;
in vec3 v_color;
uniform sampler2D u_atlas;
out vec4 frag;
void main() {
    float a = texture(u_atlas, v_uv).r;
    frag = vec4(v_color, a);
}
"#;
    unsafe {
        let program = gl.create_program().expect("program");
        for (kind, src) in [(glow::VERTEX_SHADER, VS), (glow::FRAGMENT_SHADER, FS)] {
            let shader = gl.create_shader(kind).expect("shader");
            gl.shader_source(shader, src);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                eprintln!(
                    "hyprburst-spike-glterm: shader compile error: {}",
                    gl.get_shader_info_log(shader)
                );
            }
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            eprintln!(
                "hyprburst-spike-glterm: program link error: {}",
                gl.get_program_info_log(program)
            );
        }
        program
    }
}

/// Map an `alacritty_terminal` cell color to a normalized RGB triple, with a
/// light-grey default for the palette's foreground.
fn ansi_rgb(color: AnsiColor) -> [f32; 3] {
    let (r, g, b) = match color {
        AnsiColor::Spec(rgb) => (rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(i) => indexed_rgb(i),
        AnsiColor::Named(named) => named_rgb(named),
    };
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

/// The standard xterm 256-color palette for `Indexed` cells.
fn indexed_rgb(i: u8) -> (u8, u8, u8) {
    match i {
        0..=15 => base16_rgb(i),
        16..=231 => {
            let i = i - 16;
            let steps = [0u8, 95, 135, 175, 215, 255];
            (
                steps[(i / 36) as usize],
                steps[((i / 6) % 6) as usize],
                steps[(i % 6) as usize],
            )
        }
        _ => {
            let v = 8 + 10 * (i - 232);
            (v, v, v)
        }
    }
}

/// The 16 ANSI base colors.
fn base16_rgb(i: u8) -> (u8, u8, u8) {
    match i {
        0 => (0, 0, 0),
        1 => (205, 0, 0),
        2 => (0, 205, 0),
        3 => (205, 205, 0),
        4 => (0, 0, 238),
        5 => (205, 0, 205),
        6 => (0, 205, 205),
        7 => (229, 229, 229),
        8 => (127, 127, 127),
        9 => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (92, 92, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        _ => (255, 255, 255),
    }
}

/// Map a `NamedColor` to RGB; foreground/background and unhandled names fall back
/// to a readable light grey / black.
fn named_rgb(named: NamedColor) -> (u8, u8, u8) {
    match named {
        NamedColor::Black => base16_rgb(0),
        NamedColor::Red => base16_rgb(1),
        NamedColor::Green => base16_rgb(2),
        NamedColor::Yellow => base16_rgb(3),
        NamedColor::Blue => base16_rgb(4),
        NamedColor::Magenta => base16_rgb(5),
        NamedColor::Cyan => base16_rgb(6),
        NamedColor::White => base16_rgb(7),
        NamedColor::BrightBlack => base16_rgb(8),
        NamedColor::BrightRed => base16_rgb(9),
        NamedColor::BrightGreen => base16_rgb(10),
        NamedColor::BrightYellow => base16_rgb(11),
        NamedColor::BrightBlue => base16_rgb(12),
        NamedColor::BrightMagenta => base16_rgb(13),
        NamedColor::BrightCyan => base16_rgb(14),
        NamedColor::BrightWhite => base16_rgb(15),
        NamedColor::Background => (0, 0, 0),
        _ => (229, 229, 229),
    }
}

/// Find a monospace TTF/OTF: `HYPRBURST_GLTERM_FONT` if set, else a few common
/// paths. Returns the parsed font, or `None` if none could be loaded.
fn load_monospace_font() -> Option<FontVec> {
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(path) = std::env::var("HYPRBURST_GLTERM_FONT") {
        candidates.push(path.into());
    }
    for p in [
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf",
        "/usr/share/fonts/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/TTF/Hack-Regular.ttf",
    ] {
        candidates.push(p.into());
    }
    candidates
        .into_iter()
        .find_map(|path| std::fs::read(&path).ok())
        .and_then(|bytes| FontVec::try_from_vec(bytes).ok())
}

/// Listen for `SIGUSR1` on a dedicated thread and, on each one, arm a warm-toggle
/// measurement and wake the window for a re-show repaint. The unattended driver
/// and a stack-agnostic manual fallback for the special-workspace bind.
fn spawn_show_trigger_listener(proxy: EventLoopProxy<UserEvent>) {
    use signal_hook::consts::SIGUSR1;
    use signal_hook::iterator::Signals;

    std::thread::spawn(move || match Signals::new([SIGUSR1]) {
        Ok(mut signals) => {
            for _ in signals.forever() {
                if let Some(start) = START.get() {
                    WARM.arm(start.elapsed().as_nanos() as u64);
                }
                let _ = proxy.send_event(UserEvent::Wakeup);
            }
        }
        Err(err) => eprintln!("hyprburst-spike-glterm: SIGUSR1 listener unavailable: {err}"),
    });
}

/// Print each newly-measured warm-toggle latency to stderr (resident mode).
fn spawn_warm_reporter() {
    std::thread::spawn(|| {
        let mut last = 0u64;
        loop {
            if let Some(ns) = WARM.latest()
                && ns != last
            {
                last = ns;
                eprintln!(
                    "hyprburst-spike-glterm: warm toggle {:.3} ms (show-trigger → present)",
                    ns as f64 / 1e6,
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
}

/// Print the live gl-term column: cold-start (to first presented frame) plus
/// footprint, reusing the contract the other columns use. The dep count is
/// attributed to the Skia-free `glterm-spike` feature.
fn report(cold_start_ns: u64) {
    let footprint = bench::live_footprint_for(&["glterm-spike"]);
    let metrics = [bench::probe_metrics(VARIANT, cold_start_ns, footprint)];
    print!(
        "{}\n\n{}\n",
        bench::render_table(&metrics),
        bench::to_json(&metrics)
    );
}

//! The GUI launcher window and shared GL cell renderer.
//!
//! A winit window, a hand-written OpenGL cell renderer (glutin context, glow draw,
//! and an `ab_glyph` glyph atlas), and an in-process [`LauncherCore`]. Because we
//! The default frontend paints a `rio-vt` grid backed by a PTY running
//! `hyprburst tui`. The `native` fallback paints [`render_core`] straight into a
//! ratatui [`Buffer`] without a PTY. Both paths share the same renderer, exact
//! ratatui look, and Hyprland blur/transparency behavior.
//!
//! [`run`] opens the window and drives the launcher interactively: Enter launches
//! the selected app via the core (which dispatches through `hyprctl`) and the
//! window closes; Esc cancels. With `measure = true` the process exits right
//! after the first frame is presented, printing a cold-start / peak-RSS report.
//!
//! The renderer drives the *same* tested layout/atlas primitives in
//! [`crate::gpu::grid`]; only the GL upload/draw lives here, verified in a live
//! Hyprland session.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use glow::HasContext;
use glutin::config::ConfigTemplateBuilder;
use glutin::context::{ContextAttributesBuilder, PossiblyCurrentContext};
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{Surface, SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::{Window, WindowAttributes, WindowId};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use rio_vt::crosswords::grid::row::Row;
use rio_vt::crosswords::square::Square;

use crate::bench;
use crate::domain::config::Config;
use crate::domain::launcher_core::{LauncherAction, LauncherCore};
use crate::gpu::grid::{
    Atlas, BgCell, CellInstance, CellMetrics, GlyphKey, GridSize, cell_at_pixel, cell_rect,
    grid_size,
};
use crate::gpu::rio::{KeyInput, Session};
use crate::view::layout::entry_at;
use crate::view::render::{RenderCache, render_core};

/// Atlas texture dimensions — ample for a launcher's character set.
const ATLAS_W: u32 = 2048;
const ATLAS_H: u32 = 2048;
/// Fade-in duration (seconds) on first show — an ease-out ramp of the global alpha.
const FADE_SECS: f32 = 0.20;

/// Nanoseconds from process start to the first frame actually *presented* (buffer
/// swapped) — the honest cold-start (time-to-visible). `0` = not yet painted.
static FIRST_PRESENT_NS: AtomicU64 = AtomicU64::new(0);
/// Exit the process right after the first present (`measure` mode).
static MEASURE: AtomicBool = AtomicBool::new(false);

/// Open the launcher window and run it to completion. `start` is the process's
/// reference instant (captured in `main` before any work) so the cold-start
/// report measures time-to-first-frame honestly. With `measure` the process
/// exits right after the first present, printing the cold-start / RSS report.
pub fn run(
    config: Config,
    measure: bool,
    start: Instant,
) -> Result<(), Box<dyn std::error::Error>> {
    MEASURE.store(measure, Ordering::SeqCst);
    FIRST_PRESENT_NS.store(0, Ordering::SeqCst);

    let font = match crate::gpu::font::resolve_font(config.font.path.as_deref()) {
        Some(font) => font,
        None => {
            return Err(
                "no monospace font found; set [font] path in config or $HYPRBURST_FONT to a .ttf/.otf path"
                    .into(),
            );
        }
    };

    let event_loop = EventLoop::<()>::with_user_event().build().map_err(|err| {
        format!("cannot create event loop ({err}) — is a Wayland display available?")
    })?;

    let mut app = App::new(start, font, config);
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Open the default Rio-backed frontend. Rio owns the PTY and VT grid while the
/// existing OpenGL renderer keeps ownership of the native window.
pub fn run_rio(
    config: Config,
    measure: bool,
    start: Instant,
) -> Result<(), Box<dyn std::error::Error>> {
    MEASURE.store(measure, Ordering::SeqCst);
    FIRST_PRESENT_NS.store(0, Ordering::SeqCst);

    let font = match crate::gpu::font::resolve_font(config.font.path.as_deref()) {
        Some(font) => font,
        None => {
            return Err(
                "no monospace font found; set [font] path in config or $HYPRBURST_FONT to a .ttf/.otf path"
                    .into(),
            );
        }
    };
    let executable = std::env::current_exe()?;
    let event_loop = EventLoop::<()>::with_user_event().build().map_err(|err| {
        format!("cannot create event loop ({err}) — is a Wayland display available?")
    })?;
    let proxy = event_loop.create_proxy();
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = proxy.send_event(());
    });
    let mut app = App::new_rio(start, font, config, executable, wake);
    event_loop.run_app(&mut app)?;
    Ok(())
}

enum Frontend {
    Launcher(Box<LauncherCore>),
    Rio {
        session: Option<Session>,
        executable: PathBuf,
        wake: Arc<dyn Fn() + Send + Sync>,
    },
}

/// The winit application: holds GL state once resumed, the in-process launcher,
/// the grid size, and the GUI appearance resolved from config. All window/GL
/// mutation happens on the main thread.
struct App {
    start: Instant,
    font: FontVec,
    frontend: Frontend,
    gl: Option<GlState>,
    /// Pixel size of one monospace cell, derived from the font + DPI once the
    /// window exists. A 1×1 placeholder until then.
    cell: CellMetrics,
    grid: (u16, u16),
    cursor_position: Option<(f64, f64)>,
    /// Wayland app-id the windowrules match (`[window] app_id`).
    app_id: String,
    /// Initial window size in logical pixels (`[window] width/height`).
    win_w: u32,
    win_h: u32,
    /// Transparent surface for the Hyprland blur (`[window] transparent`).
    transparent: bool,
    /// Logical font size in pixels before DPI scaling (`[font] size`).
    font_px: f32,
    /// Default/Reset foreground as normalized RGB (`[colors] foreground`).
    fg: [f32; 3],
    bg: [f32; 3],
    /// Clear color as normalized RGBA: transparent for the blur, or the opaque
    /// `[colors] background` when `transparent = false`.
    clear: [f32; 4],
    /// Dimming panel painted behind the launcher when transparent: the
    /// `[colors] background` at `[window] opacity`. `None` when the surface is
    /// already opaque (or opacity is 0).
    panel: Option<[f32; 4]>,
    /// When the fade-in began (first painted frame); `None` until then.
    fade_start: Option<Instant>,
    /// Persistent launcher buffer, reused across frames so a keystroke doesn't
    /// reallocate the whole cell grid (`None` until the first paint).
    buf: Option<Buffer>,
    /// Reused per-frame cell lists (launcher + Rio paths share them).
    bgs: Vec<BgCell>,
    glyphs: Vec<CellInstance>,
    /// Scratch rows for the Rio damage snapshot (`fill_visible_rows` target).
    rio_rows: Vec<Row<Square>>,
    /// Whether anything changed since the last painted frame. When false and
    /// the fade-in is done, an idle window skips rebuild, upload, *and* present.
    repaint_needed: bool,
    /// Shared render-string/layout cache for the launcher buffer path.
    render_cache: RenderCache,
}

impl App {
    fn new(start: Instant, font: FontVec, config: Config) -> Self {
        let frontend = Frontend::Launcher(Box::new(LauncherCore::new(config.clone())));
        Self::with_frontend(start, font, config, frontend)
    }

    fn new_rio(
        start: Instant,
        font: FontVec,
        config: Config,
        executable: PathBuf,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let frontend = Frontend::Rio {
            session: None,
            executable,
            wake,
        };
        Self::with_frontend(start, font, config, frontend)
    }

    fn with_frontend(start: Instant, font: FontVec, config: Config, frontend: Frontend) -> Self {
        let fg = rgb_norm(color_rgb(config.colors.foreground));
        let (br, bg_, bb) = color_rgb(config.colors.background);
        let bg_norm = [br as f32 / 255.0, bg_ as f32 / 255.0, bb as f32 / 255.0];
        let clear = if config.window.transparent {
            // Fully transparent so Hyprland's blur shows through the window.
            [0.0, 0.0, 0.0, 0.0]
        } else {
            [bg_norm[0], bg_norm[1], bg_norm[2], 1.0]
        };
        // When transparent, dim the blur with a background panel at the configured
        // opacity (1.0 = fully hide the blur). When opaque, the clear already fills
        // the surface, so no panel is needed.
        let panel = (config.window.transparent && config.window.opacity > 0.0).then_some([
            bg_norm[0],
            bg_norm[1],
            bg_norm[2],
            config.window.opacity,
        ]);
        Self {
            start,
            font,
            app_id: config.window.app_id.clone(),
            win_w: config.window.width,
            win_h: config.window.height,
            transparent: config.window.transparent,
            font_px: config.font.size,
            fg,
            bg: bg_norm,
            clear,
            panel,
            fade_start: None,
            buf: None,
            bgs: Vec::new(),
            glyphs: Vec::new(),
            rio_rows: Vec::new(),
            repaint_needed: true,
            render_cache: RenderCache::new(),
            frontend,
            gl: None,
            cell: CellMetrics::new(1, 1),
            grid: (1, 1),
            cursor_position: None,
        }
    }
}

/// Font-derived cell geometry for the live renderer.
struct FontMetrics {
    /// Pixel size of one monospace cell (advance width × line height).
    cell: CellMetrics,
    /// Font size in pixels (DPI-scaled) glyphs are rasterized at.
    px: f32,
    /// Baseline offset from the cell top, in pixels.
    ascent: f32,
}

/// Compute the cell geometry from the font at `base_px` scaled by the window's
/// DPI `scale_factor`: cell width = the monospace advance, cell height = ascent −
/// descent + line gap. Glyphs are rasterized at the full `px` size (no shrink) so
/// box-drawing fills the cell and the banner art connects.
fn font_metrics(font: &FontVec, scale_factor: f64, base_px: f32) -> FontMetrics {
    let px = (base_px as f64 * scale_factor.max(1.0)) as f32;
    let scaled = font.as_scaled(PxScale::from(px));
    let cell_w = scaled.h_advance(font.glyph_id('M')).ceil().max(1.0) as u32;
    let cell_h = (scaled.ascent() - scaled.descent() + scaled.line_gap())
        .ceil()
        .max(1.0) as u32;
    FontMetrics {
        cell: CellMetrics::new(cell_w, cell_h),
        px,
        ascent: scaled.ascent(),
    }
}

impl ApplicationHandler<()> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gl.is_some() {
            return;
        }

        let window_attributes = WindowAttributes::default()
            .with_title("hyprburst")
            // On Wayland the general name becomes the app-id the windowrules match.
            .with_name(&self.app_id, &self.app_id)
            .with_inner_size(winit::dpi::LogicalSize::new(self.win_w, self.win_h))
            .with_transparent(self.transparent);

        let gl = match GlState::new(event_loop, window_attributes, &self.font, self.font_px) {
            Ok(gl) => gl,
            Err(err) => {
                eprintln!("hyprburst: GL bootstrap failed: {err}");
                event_loop.exit();
                return;
            }
        };

        let size = gl.window.inner_size();
        self.cell = gl.renderer.cell;
        let grid = grid_size((size.width, size.height), self.cell);
        self.grid = (grid.cols, grid.rows);
        self.repaint_needed = true;

        match &mut self.frontend {
            Frontend::Launcher(_) => gl.window.request_redraw(),
            Frontend::Rio {
                session,
                executable,
                wake,
            } => match Session::spawn(
                executable,
                self.grid,
                (size.width, size.height),
                (self.cell.cell_w, self.cell.cell_h),
                Arc::clone(wake),
            ) {
                Ok(rio) => *session = Some(rio),
                Err(err) => {
                    eprintln!("hyprburst: Rio PTY bootstrap failed: {err}");
                    event_loop.exit();
                    return;
                }
            },
        }
        self.gl = Some(gl);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gl) = &mut self.gl {
                    gl.resize(size.width, size.height);
                }
                let grid = grid_size((size.width, size.height), self.cell);
                self.grid = (grid.cols, grid.rows);
                self.repaint_needed = true;
                if let Frontend::Rio {
                    session: Some(session),
                    ..
                } = &self.frontend
                {
                    session.resize(
                        self.grid,
                        (size.width, size.height),
                        (self.cell.cell_w, self.cell.cell_h),
                    );
                }
                if let Some(gl) = &self.gl {
                    gl.window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    match &mut self.frontend {
                        Frontend::Launcher(core) => {
                            if let Some(action) = key_to_action(&event) {
                                core.apply(action);
                                self.repaint_needed = true;
                                if !core.running() {
                                    event_loop.exit();
                                } else if let Some(gl) = &self.gl {
                                    gl.window.request_redraw();
                                }
                            }
                        }
                        Frontend::Rio {
                            session: Some(session),
                            ..
                        } => {
                            if let Some(key) = terminal_key(&event) {
                                session.send_key(key);
                            }
                        }
                        Frontend::Rio { session: None, .. } => {}
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = Some((position.x, position.y));
            }
            WindowEvent::CursorLeft { .. } => self.cursor_position = None,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let Some(position) = self.cursor_position else {
                    return;
                };
                let Some(cell) = cell_at_pixel(
                    position,
                    self.cell,
                    GridSize {
                        cols: self.grid.0,
                        rows: self.grid.1,
                    },
                ) else {
                    return;
                };

                match &mut self.frontend {
                    Frontend::Launcher(core) => {
                        let area = Rect::new(0, 0, self.grid.0, self.grid.1);
                        let entry_count = core.view().entries.len();
                        if let Some(index) = entry_at(area, core.config(), cell, entry_count) {
                            core.apply(LauncherAction::SelectEntry(index));
                            core.apply(LauncherAction::LaunchSelected);
                            event_loop.exit();
                        }
                    }
                    Frontend::Rio {
                        session: Some(session),
                        ..
                    } => session.send_mouse_press(cell.0, cell.1),
                    Frontend::Rio { session: None, .. } => {}
                }
            }
            WindowEvent::RedrawRequested => {
                if let Frontend::Rio {
                    session: Some(session),
                    ..
                } = &self.frontend
                    && session.closed()
                {
                    event_loop.exit();
                    return;
                }
                let Some(gl) = &mut self.gl else {
                    return;
                };

                // While the fade-in runs, every frame changes the global alpha.
                // Once it's done, a frame is only worth painting when something
                // actually changed — an idle window does zero work here.
                let fade_done = self
                    .fade_start
                    .is_some_and(|s| s.elapsed().as_secs_f32() >= FADE_SECS);
                if !self.repaint_needed && fade_done {
                    return;
                }

                match &mut self.frontend {
                    Frontend::Launcher(core) => {
                        build_cells(
                            core,
                            &mut self.render_cache,
                            self.grid,
                            self.fg,
                            &mut self.buf,
                            &mut self.bgs,
                            &mut self.glyphs,
                        );
                    }
                    Frontend::Rio {
                        session: Some(session),
                        ..
                    } => {
                        let changed = session.frame_into(
                            self.fg,
                            self.bg,
                            &mut self.rio_rows,
                            &mut self.bgs,
                            &mut self.glyphs,
                        );
                        if !changed && fade_done {
                            // The wake carried no visible change; skip the draw
                            // and present, and stay idle until the next event.
                            self.repaint_needed = false;
                            return;
                        }
                    }
                    Frontend::Rio { session: None, .. } => {
                        self.bgs.clear();
                        self.glyphs.clear();
                    }
                }

                // Fade-in: ease-out ramp of the global alpha over FADE_SECS,
                // measured from the first painted frame.
                let fade_start = self.fade_start.get_or_insert_with(Instant::now);
                let t = (fade_start.elapsed().as_secs_f32() / FADE_SECS).clamp(0.0, 1.0);
                let alpha = 1.0 - (1.0 - t).powi(4);
                unsafe {
                    let size = gl.window.inner_size();
                    gl.renderer.draw(
                        &gl.gl,
                        &self.font,
                        size.width,
                        size.height,
                        &self.bgs,
                        &self.glyphs,
                        self.clear,
                        self.panel,
                        alpha,
                    );
                }
                if let Err(err) = gl.surface.swap_buffers(&gl.context)
                    && !gl.swap_warned
                {
                    gl.swap_warned = true;
                    eprintln!("hyprburst: buffer swap failed: {err:?}");
                }
                self.repaint_needed = false;

                // Keep animating until the fade completes.
                if t < 1.0 {
                    gl.window.request_redraw();
                }

                // Stamp the honest cold-start at the first present.
                let ns = (self.start.elapsed().as_nanos() as u64).max(1);
                let first = FIRST_PRESENT_NS
                    .compare_exchange(0, ns, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok();
                if first && MEASURE.load(Ordering::SeqCst) {
                    let variant = match &self.frontend {
                        Frontend::Launcher(_) => "gui",
                        Frontend::Rio { .. } => "rio-vt",
                    };
                    report(variant, ns);
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, (): ()) {
        if let Frontend::Rio {
            session: Some(session),
            ..
        } = &self.frontend
            && session.closed()
        {
            event_loop.exit();
            return;
        }
        // A PTY wake means bytes may have arrived; whether they changed any
        // visible cell is decided by the damage gate at frame-build time.
        self.repaint_needed = true;
        if let Some(gl) = &self.gl {
            gl.window.request_redraw();
        }
    }
}

/// Render the launcher into a caller-owned buffer sized to the grid and collect
/// the non-blank cells the GL renderer draws into reused lists. The buffer and
/// cell lists persist across frames, so a keystroke reuses their capacity
/// instead of reallocating the whole grid.
fn build_cells(
    core: &mut LauncherCore,
    cache: &mut RenderCache,
    grid: (u16, u16),
    default_fg: [f32; 3],
    buf_slot: &mut Option<Buffer>,
    bgs: &mut Vec<BgCell>,
    glyphs: &mut Vec<CellInstance>,
) {
    let area = Rect::new(0, 0, grid.0, grid.1);
    let buf = buf_slot.get_or_insert_with(|| Buffer::empty(area));
    if buf.area != area {
        *buf = Buffer::empty(area);
    } else {
        buf.reset();
    }
    render_core(cache, core, area, buf);

    let width = area.width;
    bgs.clear();
    glyphs.clear();
    for (i, cell) in buf.content().iter().enumerate() {
        let i = i as u16;
        let (col, row) = (i % width, i / width);

        // Background fill: any cell with a non-default bg (e.g. the selected row,
        // whose bar spans its spaces too) gets a solid quad behind the glyphs.
        if !matches!(cell.bg, Color::Reset) {
            let (r, g, b) = color_rgb(cell.bg);
            bgs.push(BgCell {
                col,
                row,
                color: [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0],
            });
        }

        let ch = cell.symbol().chars().next().unwrap_or(' ');
        if ch != ' ' && ch != '\0' {
            glyphs.push(CellInstance {
                col,
                row,
                ch,
                bold: cell.modifier.contains(Modifier::BOLD),
                color: ratatui_rgb(cell.fg, default_fg),
            });
        }
    }
}

/// Translate a winit key press into an abstract [`LauncherAction`] — the same
/// vocabulary the crossterm TUI maps. Keys with no launcher meaning return `None`.
fn key_to_action(event: &KeyEvent) -> Option<LauncherAction> {
    Some(match &event.logical_key {
        Key::Named(NamedKey::Enter) => LauncherAction::LaunchSelected,
        Key::Named(NamedKey::Escape) => LauncherAction::Cancel,
        Key::Named(NamedKey::Tab) => LauncherAction::Autocomplete,
        Key::Named(NamedKey::Backspace) => LauncherAction::Backspace,
        Key::Named(NamedKey::PageUp) => LauncherAction::PageUp,
        Key::Named(NamedKey::PageDown) => LauncherAction::PageDown,
        Key::Named(NamedKey::ArrowUp) => LauncherAction::MoveUp,
        Key::Named(NamedKey::ArrowDown) => LauncherAction::MoveDown,
        Key::Named(NamedKey::ArrowLeft) => LauncherAction::MoveLeft,
        Key::Named(NamedKey::ArrowRight) => LauncherAction::MoveRight,
        Key::Named(NamedKey::Space) => LauncherAction::Insert(' '),
        _ => {
            let text = event.text.as_ref()?;
            let ch = text.chars().next().filter(|c| !c.is_control())?;
            LauncherAction::Insert(ch)
        }
    })
}

fn terminal_key(event: &KeyEvent) -> Option<KeyInput> {
    Some(match &event.logical_key {
        Key::Named(NamedKey::Enter) => KeyInput::Enter,
        Key::Named(NamedKey::Escape) => KeyInput::Escape,
        Key::Named(NamedKey::Tab) => KeyInput::Tab,
        Key::Named(NamedKey::Backspace) => KeyInput::Backspace,
        Key::Named(NamedKey::PageUp) => KeyInput::PageUp,
        Key::Named(NamedKey::PageDown) => KeyInput::PageDown,
        Key::Named(NamedKey::ArrowUp) => KeyInput::Up,
        Key::Named(NamedKey::ArrowDown) => KeyInput::Down,
        Key::Named(NamedKey::ArrowLeft) => KeyInput::Left,
        Key::Named(NamedKey::ArrowRight) => KeyInput::Right,
        Key::Named(NamedKey::Space) => KeyInput::Text(" ".to_string()),
        _ => KeyInput::Text(event.text.as_ref()?.to_string()),
    })
}

/// GL state realized after the window is created: the surface, the current
/// context, the glow context, and the cell renderer.
struct GlState {
    window: Window,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    gl: glow::Context,
    renderer: CellRenderer,
    /// Set after the first `swap_buffers` failure is reported, so a persistently
    /// broken surface logs once instead of spamming every frame.
    swap_warned: bool,
}

impl GlState {
    fn new(
        event_loop: &ActiveEventLoop,
        window_attributes: WindowAttributes,
        font: &FontVec,
        font_px: f32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_transparency(true);
        let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attributes));

        let (window, gl_config) = display_builder.build(event_loop, template, |configs| {
            // Prefer a config with the most alpha bits (for the blur windowrule).
            // (The glutin callback API can only signal "no configs" by panicking;
            // a driver offering zero configs fails context creation anyway.)
            configs
                .reduce(|acc, cfg| {
                    if cfg.alpha_size() > acc.alpha_size() {
                        cfg
                    } else {
                        acc
                    }
                })
                .expect("no GL configs offered by the driver")
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

        let fm = font_metrics(font, window.scale_factor(), font_px);
        let renderer = unsafe { CellRenderer::new(&gl, &fm)? };

        Ok(Self {
            window,
            surface,
            context,
            gl,
            renderer,
            swap_warned: false,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if let (Some(w), Some(h)) = (NonZeroU32::new(width), NonZeroU32::new(height)) {
            self.surface.resize(&self.context, w, h);
            unsafe { self.gl.viewport(0, 0, width as i32, height as i32) };
        }
    }
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
    alpha_loc: Option<glow::UniformLocation>,
    atlas_tex: glow::Texture,
    atlas: Atlas,
    /// UV of the reserved fully-opaque texel that solid (panel/selection) quads
    /// sample so they share the glyph shader and draw call.
    white_uv: (f32, f32),
    /// Pixel size of one cell — the atlas tile size and the layout unit.
    cell: CellMetrics,
    scale: ab_glyph::PxScale,
    ascent: f32,
    /// Vertex scratch reused across frames; capacity grows once and stays.
    verts: Vec<f32>,
}

/// Append one vertex (pos.xy px, uv.xy, color.rgba) to the buffer.
fn push_vert(buf: &mut Vec<f32>, x: f32, y: f32, u: f32, v: f32, color: [f32; 4]) {
    buf.extend_from_slice(&[x, y, u, v, color[0], color[1], color[2], color[3]]);
}

/// Append a solid quad (`color` over the rect, sampling the atlas's white texel so
/// the shared shader fills it flat) to the vertex buffer.
fn push_solid_quad(
    buf: &mut Vec<f32>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    uv: (f32, f32),
    color: [f32; 4],
) {
    let (u, v) = uv;
    push_vert(buf, x0, y0, u, v, color);
    push_vert(buf, x1, y0, u, v, color);
    push_vert(buf, x1, y1, u, v, color);
    push_vert(buf, x0, y0, u, v, color);
    push_vert(buf, x1, y1, u, v, color);
    push_vert(buf, x0, y1, u, v, color);
}

/// Floats per vertex: pos.xy (px), uv.xy, color.rgba.
const VERTEX_FLOATS: usize = 8;

impl CellRenderer {
    /// # Safety
    /// `gl` must be a current context for the lifetime of the renderer.
    ///
    /// Returns `Err` instead of panicking on GL object-creation or shader
    /// failures — those are driver-dependent conditions, not logic bugs, and
    /// the caller surfaces them as a clean `GL bootstrap failed` exit.
    unsafe fn new(gl: &glow::Context, fm: &FontMetrics) -> Result<Self, String> {
        let program = unsafe { build_program(gl)? };
        let vao = unsafe {
            gl.create_vertex_array()
                .map_err(|e| format!("create VAO: {e:?}"))?
        };
        let vbo = unsafe {
            gl.create_buffer()
                .map_err(|e| format!("create VBO: {e:?}"))?
        };
        let viewport_loc = unsafe { gl.get_uniform_location(program, "u_viewport") };
        let atlas_loc = unsafe { gl.get_uniform_location(program, "u_atlas") };
        let alpha_loc = unsafe { gl.get_uniform_location(program, "u_alpha") };

        // Reserve the first atlas slot for a fully-opaque white texel that solid
        // quads (the background panel and the selection bar) sample, so they reuse
        // the glyph shader and draw call. The sentinel key never collides with a
        // real glyph; real glyphs take slots 1+.
        let mut atlas = Atlas::new((ATLAS_W, ATLAS_H), fm.cell);
        let Some(white_slot) = atlas.get_or_insert(GlyphKey::new('\0')) else {
            return Err("atlas has no room for the white texel".into());
        };
        let (wu0, wv0, wu1, wv1) = atlas.uv_rect(white_slot);
        let white_uv = ((wu0 + wu1) * 0.5, (wv0 + wv1) * 0.5);

        let atlas_tex = unsafe {
            gl.create_texture()
                .map_err(|e| format!("create atlas texture: {e:?}"))?
        };
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(atlas_tex));
            // Byte-aligned unpack rows BEFORE any upload. The white-tile fill
            // below is `cell_w` pixels wide, which need not be a multiple of
            // GL's default 4-byte row alignment; uploading under the default
            // makes the driver read past the buffer's end.
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
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
            // Fill the reserved white tile (slot 0) with full coverage.
            let (cw, chh) = (fm.cell.cell_w, fm.cell.cell_h);
            // Checked: this product backs the upload buffer; a metrics bug
            // must not wrap it into an undersized slice for GL to read past.
            if let Some(len) = cw.checked_mul(chh) {
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    white_slot.px.0 as i32,
                    white_slot.px.1 as i32,
                    cw as i32,
                    chh as i32,
                    glow::RED,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&vec![255u8; len as usize])),
                );
            } else {
                eprintln!("hyprburst: cell metrics {cw}x{chh} overflow; skipping white-tile fill");
            }
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
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        }

        Ok(Self {
            program,
            vao,
            vbo,
            viewport_loc,
            atlas_loc,
            alpha_loc,
            atlas_tex,
            atlas,
            white_uv,
            cell: fm.cell,
            scale: PxScale::from(fm.px),
            ascent: fm.ascent,
            verts: Vec::new(),
        })
    }

    /// Ensure `ch` is resident in the atlas, rasterizing + uploading it on
    /// first sight. Returns its normalized UV rect, or `None` if the atlas is full.
    fn ensure_glyph(
        &mut self,
        gl: &glow::Context,
        font: &FontVec,
        ch: char,
    ) -> Option<(f32, f32, f32, f32)> {
        let slot = self.atlas.get_or_insert(GlyphKey::new(ch))?;
        if slot.newly_inserted {
            self.rasterize_into(gl, font, ch, slot.px);
        }
        Some(self.atlas.uv_rect(slot))
    }

    /// Rasterize `ch` into the atlas tile at pixel origin `px` via `ab_glyph`.
    fn rasterize_into(&self, gl: &glow::Context, font: &FontVec, ch: char, px: (u32, u32)) {
        let (cw, ch_px) = (self.cell.cell_w, self.cell.cell_h);
        // Checked: this product is both the coverage buffer length and the
        // upload's declared pixel count; a metrics bug must not wrap it into
        // an undersized slice for GL to read past.
        let Some(buf_len) = cw.checked_mul(ch_px) else {
            return;
        };
        let mut coverage = vec![0u8; buf_len as usize];
        let glyph = font
            .glyph_id(ch)
            .with_scale_and_position(self.scale, ab_glyph::point(0.0, self.ascent));
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, c| {
                let x = gx as i32 + bounds.min.x as i32;
                let y = gy as i32 + bounds.min.y as i32;
                if x >= 0 && (x as u32) < cw && y >= 0 && (y as u32) < ch_px {
                    // usize math: no u32 intermediate can overflow here since
                    // y < ch_px and x < cw and cw × ch_px fits (checked above).
                    let idx = y as usize * cw as usize + x as usize;
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
                cw as i32,
                ch_px as i32,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&coverage)),
            );
        }
    }

    /// Draw a frame in a single call: a full-window dimming `panel` (if any), then
    /// the background fills (selection bar), then the glyph quads — appended in
    /// that order so `SRC_ALPHA` blending layers correctly — uploaded once and
    /// drawn with `glDrawArrays`. This is the only place with the live GL context,
    /// so glyph upload happens here. `clear` is the RGBA the frame is cleared to
    /// (transparent for the blur, or an opaque background); `alpha` is the global
    /// fade-in factor multiplied into every quad.
    ///
    /// # Safety
    /// `gl` must be the current context.
    #[allow(clippy::too_many_arguments)]
    /// Build the frame's vertices from the cell lists and draw them in one call.
    /// Only called when a repaint is actually needed (see `App::repaint_needed`):
    /// on change we re-orphan + refill the VBO via `buffer_data`, the standard
    /// driver-friendly dynamic path, instead of sub-updating variable slices.
    #[allow(clippy::too_many_arguments)]
    unsafe fn draw(
        &mut self,
        gl: &glow::Context,
        font: &FontVec,
        width: u32,
        height: u32,
        bgs: &[BgCell],
        glyphs: &[CellInstance],
        clear: [f32; 4],
        panel: Option<[f32; 4]>,
        alpha: f32,
    ) {
        let cell = self.cell;
        let white = self.white_uv;
        self.verts.clear();

        // 1. Full-window dimming panel, behind everything.
        if let Some(color) = panel {
            push_solid_quad(
                &mut self.verts,
                0.0,
                0.0,
                width as f32,
                height as f32,
                white,
                color,
            );
        }
        // 2. Per-cell background fills (the selection bar).
        for b in bgs {
            let (x, y, w, h) = cell_rect(b.col, b.row, cell);
            push_solid_quad(
                &mut self.verts,
                x as f32,
                y as f32,
                (x + w) as f32,
                (y + h) as f32,
                white,
                b.color,
            );
        }
        // 3. Glyph quads on top. The verts borrow is taken per push so it never
        // overlaps `ensure_glyph`'s `&mut self` (atlas insertion).
        for c in glyphs {
            let Some((u0, v0, u1, v1)) = self.ensure_glyph(gl, font, c.ch) else {
                continue;
            };
            let (x, y, w, h) = cell_rect(c.col, c.row, cell);
            let (x0, y0, x1, y1) = (x as f32, y as f32, (x + w) as f32, (y + h) as f32);
            let col = [c.color[0], c.color[1], c.color[2], 1.0];
            push_vert(&mut self.verts, x0, y0, u0, v0, col);
            push_vert(&mut self.verts, x1, y0, u1, v0, col);
            push_vert(&mut self.verts, x1, y1, u1, v1, col);
            push_vert(&mut self.verts, x0, y0, u0, v0, col);
            push_vert(&mut self.verts, x1, y1, u1, v1, col);
            push_vert(&mut self.verts, x0, y1, u0, v1, col);
        }

        unsafe {
            gl.viewport(0, 0, width as i32, height as i32);
            gl.clear_color(clear[0], clear[1], clear[2], clear[3]);
            gl.clear(glow::COLOR_BUFFER_BIT);

            if self.verts.is_empty() {
                return;
            }

            gl.use_program(Some(self.program));
            gl.uniform_2_f32(self.viewport_loc.as_ref(), width as f32, height as f32);
            gl.uniform_1_f32(self.alpha_loc.as_ref(), alpha);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas_tex));
            gl.uniform_1_i32(self.atlas_loc.as_ref(), 0);

            gl.bind_vertex_array(Some(self.vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            let verts = &self.verts;
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
            gl.vertex_attrib_pointer_f32(2, 4, glow::FLOAT, false, stride, 4 * 4);

            let count = (verts.len() / VERTEX_FLOATS) as i32;
            gl.draw_arrays(glow::TRIANGLES, 0, count);
        }
    }
}

/// Compile the textured-quad shader program. Returns `Err` with the info log
/// when compilation or linking fails, so a driver problem surfaces as a clean
/// bootstrap error instead of silently drawing nothing.
///
/// # Safety
/// `gl` must be a current context.
unsafe fn build_program(gl: &glow::Context) -> Result<glow::Program, String> {
    const VS: &str = r#"#version 330 core
layout (location = 0) in vec2 a_pos;
layout (location = 1) in vec2 a_uv;
layout (location = 2) in vec4 a_color;
uniform vec2 u_viewport;
out vec2 v_uv;
out vec4 v_color;
void main() {
    vec2 ndc = vec2(a_pos.x / u_viewport.x * 2.0 - 1.0, 1.0 - a_pos.y / u_viewport.y * 2.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_uv = a_uv;
    v_color = a_color;
}
"#;
    // Glyph quads sample coverage from the atlas red channel; solid quads (panel,
    // selection bar) sample the reserved white texel (coverage 1.0). Both fold in
    // the vertex alpha and the global `u_alpha` fade-in factor.
    const FS: &str = r#"#version 330 core
in vec2 v_uv;
in vec4 v_color;
uniform sampler2D u_atlas;
uniform float u_alpha;
out vec4 frag;
void main() {
    float a = texture(u_atlas, v_uv).r;
    frag = vec4(v_color.rgb, v_color.a * a * u_alpha);
}
"#;
    unsafe {
        let program = gl
            .create_program()
            .map_err(|e| format!("create program: {e:?}"))?;
        for (kind, name, src) in [
            (glow::VERTEX_SHADER, "vertex", VS),
            (glow::FRAGMENT_SHADER, "fragment", FS),
        ] {
            let shader = gl
                .create_shader(kind)
                .map_err(|e| format!("create shader: {e:?}"))?;
            gl.shader_source(shader, src);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                return Err(format!(
                    "{} shader compile error: {}",
                    name,
                    gl.get_shader_info_log(shader)
                ));
            }
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(format!(
                "program link error: {}",
                gl.get_program_info_log(program)
            ));
        }
        Ok(program)
    }
}

/// Map a ratatui [`Color`] to a normalized RGB triple for the shader tint.
/// `Color::Reset` (the unstyled launcher text) renders as `default_fg` — the
/// configured `[colors] foreground` — so app names read correctly on the
/// background; explicit accent colors (banner, prompt, selection) come as set.
fn ratatui_rgb(color: Color, default_fg: [f32; 3]) -> [f32; 3] {
    if matches!(color, Color::Reset) {
        return default_fg;
    }
    rgb_norm(color_rgb(color))
}

/// Normalize an 8-bit RGB triple to floats in `[0, 1]`.
fn rgb_norm((r, g, b): (u8, u8, u8)) -> [f32; 3] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

/// Resolve a ratatui [`Color`] to an 8-bit RGB triple. `Reset` maps to a light
/// grey here; callers that want the configured default foreground special-case
/// `Reset` before calling this.
fn color_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(i) => indexed_rgb(i),
        Color::Reset => (220, 220, 220),
        Color::Black => (0, 0, 0),
        Color::Red => (205, 0, 0),
        Color::Green => (0, 205, 0),
        Color::Yellow => (205, 205, 0),
        Color::Blue => (0, 0, 238),
        Color::Magenta => (205, 0, 205),
        Color::Cyan => (0, 205, 205),
        Color::Gray => (229, 229, 229),
        Color::DarkGray => (127, 127, 127),
        Color::LightRed => (255, 0, 0),
        Color::LightGreen => (0, 255, 0),
        Color::LightYellow => (255, 255, 0),
        Color::LightBlue => (92, 92, 255),
        Color::LightMagenta => (255, 0, 255),
        Color::LightCyan => (0, 255, 255),
        Color::White => (255, 255, 255),
    }
}

/// The standard xterm 256-color palette for `Indexed` colors.
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

/// Print the live `gui` column: cold-start (to first presented frame) plus
/// footprint, reusing the contract the baseline column uses.
fn report(variant: &str, cold_start_ns: u64) {
    let footprint = bench::live_footprint();
    let metrics = [bench::probe_metrics(variant, cold_start_ns, footprint)];
    print!(
        "{}\n\n{}\n",
        bench::render_table(&metrics),
        bench::to_json(&metrics)
    );
}

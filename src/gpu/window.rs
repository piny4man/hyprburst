//! The GUI launcher window: a GL-rendered launcher that owns its surface without
//! a terminal emulator.
//!
//! A winit window, a hand-written OpenGL cell renderer (glutin context, glow draw,
//! and an `ab_glyph` glyph atlas), and an in-process [`LauncherCore`]. Because we
//! own the TUI, the launcher is painted with [`render_core`] straight into a
//! ratatui [`Buffer`] and that buffer's cells are drawn directly — no PTY, no
//! child process, no terminal-emulator round-trip. This is the native-GUI speed
//! with the exact ratatui look (banner, prompt, list, selection marker, accent
//! colors), and it lets Hyprland's blur/transparency show through.
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
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::wayland::WindowAttributesExtWayland;
use winit::window::{Window, WindowAttributes, WindowId};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

use crate::bench;
use crate::domain::config::Config;
use crate::domain::launcher_core::{LauncherAction, LauncherCore};
use crate::gpu::grid::{Atlas, CellMetrics, GlyphKey, cell_rect, grid_size};
use crate::view::render::render_core;

/// Column label this renderer fills in the benchmark table.
const VARIANT: &str = "gui";
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

    let font = match crate::gpu::font::resolve_font_bytes(config.font.path.as_deref())
        .and_then(|bytes| FontVec::try_from_vec(bytes).ok())
    {
        Some(font) => font,
        None => {
            return Err(
                "no monospace font found; set [font] path in config or $HYPRBURST_FONT to a .ttf/.otf path"
                    .into(),
            );
        }
    };

    let event_loop = EventLoop::new().map_err(|err| {
        format!("cannot create event loop ({err}) — is a Wayland display available?")
    })?;

    let mut app = App::new(start, font, config);
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// The winit application: holds GL state once resumed, the in-process launcher,
/// the grid size, and the GUI appearance resolved from config. All window/GL
/// mutation happens on the main thread.
struct App {
    start: Instant,
    font: FontVec,
    core: LauncherCore,
    gl: Option<GlState>,
    /// Pixel size of one monospace cell, derived from the font + DPI once the
    /// window exists. A 1×1 placeholder until then.
    cell: CellMetrics,
    grid: (u16, u16),
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
    /// Clear color as normalized RGBA: transparent for the blur, or the opaque
    /// `[colors] background` when `transparent = false`.
    clear: [f32; 4],
    /// Dimming panel painted behind the launcher when transparent: the
    /// `[colors] background` at `[window] opacity`. `None` when the surface is
    /// already opaque (or opacity is 0).
    panel: Option<[f32; 4]>,
    /// When the fade-in began (first painted frame); `None` until then.
    fade_start: Option<Instant>,
}

impl App {
    fn new(start: Instant, font: FontVec, config: Config) -> Self {
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
            clear,
            panel,
            fade_start: None,
            core: LauncherCore::new(config),
            gl: None,
            cell: CellMetrics::new(1, 1),
            grid: (1, 1),
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

impl ApplicationHandler for App {
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

        gl.window.request_redraw();
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
                if let Some(gl) = &self.gl {
                    gl.window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed
                    && let Some(action) = key_to_action(&event)
                {
                    self.core.apply(action);
                    if !self.core.running() {
                        // Enter launched (the core dispatched `hyprctl`) or Esc
                        // cancelled — either way, tear down the window.
                        event_loop.exit();
                    } else if let Some(gl) = &self.gl {
                        gl.window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(gl) = &mut self.gl {
                    let frame = build_cells(&mut self.core, self.grid, self.fg);
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
                            &frame,
                            self.clear,
                            self.panel,
                            alpha,
                        );
                    }
                    let _ = gl.surface.swap_buffers(&gl.context);

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
                        report(ns);
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }
}

/// Render the launcher into a buffer sized to the grid and collect the non-blank
/// cells the GL renderer draws. This is the live analog of the headless
/// [`crate::gpu::grid::GuiModel`] paint, minus the dirty diff (the live renderer
/// redraws the whole frame each time — the launcher repaints only on input).
fn build_cells(core: &mut LauncherCore, grid: (u16, u16), default_fg: [f32; 3]) -> Frame {
    let area = Rect::new(0, 0, grid.0, grid.1);
    let mut buf = Buffer::empty(area);
    render_core(core, area, &mut buf);

    let width = area.width;
    let mut bgs = Vec::new();
    let mut glyphs = Vec::new();
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
    Frame { bgs, glyphs }
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
        font_px: f32,
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

        let fm = font_metrics(font, window.scale_factor(), font_px);
        let renderer = unsafe { CellRenderer::new(&gl, &fm) };

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
}

/// One non-blank cell to draw, resolved from the rendered launcher buffer.
struct CellInstance {
    col: u16,
    row: u16,
    ch: char,
    bold: bool,
    color: [f32; 3],
}

/// A cell with a non-default background — a solid quad drawn behind the glyphs
/// (e.g. the selection highlight bar, which spans the row including its spaces).
struct BgCell {
    col: u16,
    row: u16,
    color: [f32; 4],
}

/// One painted frame: the background fills (drawn first) and the glyph cells.
struct Frame {
    bgs: Vec<BgCell>,
    glyphs: Vec<CellInstance>,
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
    unsafe fn new(gl: &glow::Context, fm: &FontMetrics) -> Self {
        let program = unsafe { build_program(gl) };
        let vao = unsafe { gl.create_vertex_array().expect("vao") };
        let vbo = unsafe { gl.create_buffer().expect("vbo") };
        let viewport_loc = unsafe { gl.get_uniform_location(program, "u_viewport") };
        let atlas_loc = unsafe { gl.get_uniform_location(program, "u_atlas") };
        let alpha_loc = unsafe { gl.get_uniform_location(program, "u_alpha") };

        // Reserve the first atlas slot for a fully-opaque white texel that solid
        // quads (the background panel and the selection bar) sample, so they reuse
        // the glyph shader and draw call. The sentinel key never collides with a
        // real glyph; real glyphs take slots 1+.
        let mut atlas = Atlas::new((ATLAS_W, ATLAS_H), fm.cell);
        let white_slot = atlas
            .get_or_insert(GlyphKey::new('\0', false))
            .expect("atlas has room for the white texel");
        let (wu0, wv0, wu1, wv1) = atlas.uv_rect(white_slot);
        let white_uv = ((wu0 + wu1) * 0.5, (wv0 + wv1) * 0.5);

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
            // Fill the reserved white tile (slot 0) with full coverage.
            let (cw, chh) = (fm.cell.cell_w, fm.cell.cell_h);
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                white_slot.px.0 as i32,
                white_slot.px.1 as i32,
                cw as i32,
                chh as i32,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&vec![255u8; (cw * chh) as usize])),
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

        Self {
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
        let (cw, ch_px) = (self.cell.cell_w, self.cell.cell_h);
        let mut coverage = vec![0u8; (cw * ch_px) as usize];
        let glyph = font
            .glyph_id(ch)
            .with_scale_and_position(self.scale, ab_glyph::point(0.0, self.ascent));
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, c| {
                let x = gx as i32 + bounds.min.x as i32;
                let y = gy as i32 + bounds.min.y as i32;
                if x >= 0 && (x as u32) < cw && y >= 0 && (y as u32) < ch_px {
                    let idx = (y as u32 * cw + x as u32) as usize;
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
    unsafe fn draw(
        &mut self,
        gl: &glow::Context,
        font: &FontVec,
        width: u32,
        height: u32,
        frame: &Frame,
        clear: [f32; 4],
        panel: Option<[f32; 4]>,
        alpha: f32,
    ) {
        let cell = self.cell;
        let white = self.white_uv;
        let quads = frame.glyphs.len() + frame.bgs.len() + 1;
        let mut verts: Vec<f32> = Vec::with_capacity(quads * 6 * VERTEX_FLOATS);

        // 1. Full-window dimming panel, behind everything.
        if let Some(color) = panel {
            push_solid_quad(
                &mut verts,
                0.0,
                0.0,
                width as f32,
                height as f32,
                white,
                color,
            );
        }
        // 2. Per-cell background fills (the selection bar).
        for b in &frame.bgs {
            let (x, y, w, h) = cell_rect(b.col, b.row, cell);
            push_solid_quad(
                &mut verts,
                x as f32,
                y as f32,
                (x + w) as f32,
                (y + h) as f32,
                white,
                b.color,
            );
        }
        // 3. Glyph quads on top.
        for c in &frame.glyphs {
            let Some((u0, v0, u1, v1)) = self.ensure_glyph(gl, font, c.ch, c.bold) else {
                continue;
            };
            let (x, y, w, h) = cell_rect(c.col, c.row, cell);
            let (x0, y0, x1, y1) = (x as f32, y as f32, (x + w) as f32, (y + h) as f32);
            let col = [c.color[0], c.color[1], c.color[2], 1.0];
            push_vert(&mut verts, x0, y0, u0, v0, col);
            push_vert(&mut verts, x1, y0, u1, v0, col);
            push_vert(&mut verts, x1, y1, u1, v1, col);
            push_vert(&mut verts, x0, y0, u0, v0, col);
            push_vert(&mut verts, x1, y1, u1, v1, col);
            push_vert(&mut verts, x0, y1, u0, v1, col);
        }

        unsafe {
            gl.viewport(0, 0, width as i32, height as i32);
            gl.clear_color(clear[0], clear[1], clear[2], clear[3]);
            gl.clear(glow::COLOR_BUFFER_BIT);

            if verts.is_empty() {
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

/// Compile the textured-quad shader program.
///
/// # Safety
/// `gl` must be a current context.
unsafe fn build_program(gl: &glow::Context) -> glow::Program {
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
        let program = gl.create_program().expect("program");
        for (kind, src) in [(glow::VERTEX_SHADER, VS), (glow::FRAGMENT_SHADER, FS)] {
            let shader = gl.create_shader(kind).expect("shader");
            gl.shader_source(shader, src);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                eprintln!(
                    "hyprburst: shader compile error: {}",
                    gl.get_shader_info_log(shader)
                );
            }
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            eprintln!(
                "hyprburst: program link error: {}",
                gl.get_program_info_log(program)
            );
        }
        program
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
fn report(cold_start_ns: u64) {
    let footprint = bench::live_footprint();
    let metrics = [bench::probe_metrics(VARIANT, cold_start_ns, footprint)];
    print!(
        "{}\n\n{}\n",
        bench::render_table(&metrics),
        bench::to_json(&metrics)
    );
}

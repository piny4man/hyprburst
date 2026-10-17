//! Monospace-font resolution for the GUI launcher.
//!
//! Both GPU frontends retain the requested primary text font and search fallback
//! fonts for missing glyphs. Private-use icons are fitted across two primary
//! cells when the next cell is blank (one cell otherwise), so the icon, not the
//! text face's advance width, sets their size; without an icon font, a
//! procedural four-tile app symbol avoids missing-glyph boxes. The `tui` fallback still uses the hosting terminal's font.
//! Resolution order, most specific first:
//!
//! 1. an explicit path from `[font] path` in the config,
//! 2. the `HYPRBURST_FONT` environment variable (a `.ttf`/`.otf` path),
//! 3. `[font] family` (or the shared Swatches theme family) via `fc-match`,
//! 4. fontconfig candidates: the default monospace if it is already a Nerd Font,
//!    otherwise any installed Nerd Font (a `Mono` variant preferred), then the
//!    plain default monospace (may lack icon glyphs),
//! 5. a few common hard-coded paths, as a last resort.
//!
//! Cold-start matters here (fontconfig spawns cost ~10–30 ms per open):
//!
//! - when the first parseable font from 1–2 already has Nerd Font icons,
//!   fontconfig is skipped entirely and only the hard-coded paths that exist
//!   are kept as text fallbacks;
//! - otherwise the fontconfig answers (3–4) are cached in
//!   `$XDG_CACHE_HOME/hyprburst/fonts`, keyed by the inputs and the fontconfig
//!   cache directories' mtimes, and looked up again only when the key changes
//!   or a cached font no longer exists. Uncached, the common case (a Nerd Font
//!   as the default monospace) is one `fc-match` spawn; the
//!   scan-for-any-Nerd-Font path adds one `fc-list`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use ab_glyph::{Font, FontVec, PxScale};

use crate::gpu::grid::GlyphSpan;

/// Immutable font selection for one GUI session; text geometry always uses primary.
pub(crate) struct FontSet {
    pub(crate) primary: FontVec,
    fallbacks: Vec<FontVec>,
}

impl FontSet {
    fn glyph_font(&self, ch: char) -> Option<&FontVec> {
        // Missing glyphs must never be handed to the rasterizer as glyph zero.
        std::iter::once(&self.primary)
            .chain(&self.fallbacks)
            .find(|font| font.glyph_id(ch).0 != 0)
    }

    /// One-cell coverage of `ch`: the span every non-icon cell uses.
    #[cfg(test)]
    pub(crate) fn coverage(
        &self,
        ch: char,
        size: (u32, u32),
        scale: PxScale,
        ascent: f32,
    ) -> Vec<u8> {
        self.coverage_span(ch, GlyphSpan::Single, size, scale, ascent)
    }

    /// One `size` tile of `ch`. For an icon, `Left`/`Right` are the halves of
    /// the icon fitted across two cells; text ignores the span, and without an
    /// icon font the square badge stays in the left cell.
    pub(crate) fn coverage_span(
        &self,
        ch: char,
        span: GlyphSpan,
        size: (u32, u32),
        scale: PxScale,
        ascent: f32,
    ) -> Vec<u8> {
        let (cw, height) = size;
        let split = match span {
            GlyphSpan::Single => None,
            _ if !is_icon(ch) => None,
            _ if self.glyph_font(ch).is_none() => {
                return match span {
                    GlyphSpan::Right => vec![0; (cw * height) as usize],
                    _ => self.rasterize(ch, size, scale, ascent),
                };
            }
            GlyphSpan::Left => Some(0),
            GlyphSpan::Right => Some(cw as usize),
        };
        let Some(offset) = split else {
            return self.rasterize(ch, size, scale, ascent);
        };
        let wide_w = cw.checked_mul(2).expect("cell size overflow");
        let wide = self.rasterize(ch, (wide_w, height), scale, ascent);
        wide.chunks_exact(wide_w as usize)
            .flat_map(|row| &row[offset..offset + cw as usize])
            .copied()
            .collect()
    }

    /// Rasterize `ch` into one `size` tile; icons are fitted to the whole tile.
    fn rasterize(&self, ch: char, size: (u32, u32), scale: PxScale, ascent: f32) -> Vec<u8> {
        let (cw, height) = size;
        let mut coverage = vec![0; cw.checked_mul(height).expect("cell size overflow") as usize];
        let icon = is_icon(ch);
        let Some(font) = self.glyph_font(ch) else {
            if icon {
                // A font-independent, generic app badge: four filled tiles, not
                // a .notdef box. This also works with symbol-only primary fonts.
                let side = cw.min(height).saturating_sub(2);
                let tile = side / 3;
                let left = (cw - side) / 2;
                let top = (height - side) / 2;
                for y in 0..side {
                    for x in 0..side {
                        if (x < tile || x >= side - tile) && (y < tile || y >= side - tile) {
                            coverage[((top + y) * cw + left + x) as usize] = 255;
                        }
                    }
                }
            }
            return coverage;
        };
        let mut glyph = font
            .glyph_id(ch)
            .with_scale_and_position(scale, ab_glyph::point(0.0, ascent));
        if icon {
            let Some(raw) = font.outline(glyph.id) else {
                return coverage;
            };
            // Use unscaled ink bounds, not the fallback's advance or line height:
            // Nerd Font faces may have very different metrics from the text face.
            // Fit both up and down, preserving aspect ratio. Reserve edge pixels
            // plus rounding slack for the rasterizer's outward-rounded bounds.
            let fit = (cw.saturating_sub(3) as f32 / raw.bounds.width())
                .min(height.saturating_sub(3) as f32 / raw.bounds.height().abs());
            glyph.scale = PxScale::from(fit * font.height_unscaled());
            glyph.position = ab_glyph::point(0.0, 0.0);
        }
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            let (origin_x, origin_y) = if icon {
                (
                    ((cw as f32 - bounds.width()) / 2.0).floor() as i32,
                    ((height as f32 - bounds.height()) / 2.0).floor() as i32,
                )
            } else {
                (bounds.min.x as i32, bounds.min.y as i32)
            };
            outline.draw(|gx, gy, c| {
                let x = gx as i32 + origin_x;
                let y = gy as i32 + origin_y;
                if x >= 0 && (x as u32) < cw && y >= 0 && (y as u32) < height {
                    coverage[y as usize * cw as usize + x as usize] = (c * 255.0) as u8;
                }
            });
        }
        coverage
    }
}

/// Nerd Font icons occupy Unicode's private-use areas (including supplementary).
pub(crate) fn is_icon(ch: char) -> bool {
    matches!(ch as u32, 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd)
}

/// Resolve once per window. Keeping the font set immutable means atlas keys can
/// remain characters: a given character always selects the same face and glyph.
#[hotpath::measure]
pub(crate) fn resolve_fonts(config_path: Option<&str>, family: Option<&str>) -> Option<FontSet> {
    let env_path = std::env::var(FONT_ENV).ok();
    let fallbacks: Vec<PathBuf> = FALLBACK_PATHS.iter().map(PathBuf::from).collect();
    resolve_with(config_path, env_path.as_deref(), &fallbacks, || {
        cached_fontconfig_paths(config_path, env_path.as_deref(), family)
    })
}

/// A Font Awesome icon every Nerd Font carries and the launcher draws
/// (nf-fa-firefox). A configured font that has it needs no fontconfig-derived
/// icon fallback.
const ICON_PROBE: char = '\u{f269}';

fn has_icons(font: &FontVec) -> bool {
    font.glyph_id(ICON_PROBE).0 != 0
}

/// Resolve from the explicit `config_path` / `env_path` fonts, consulting
/// `fontconfig` only when the first parseable explicit font lacks icons.
fn resolve_with(
    config_path: Option<&str>,
    env_path: Option<&str>,
    fallbacks: &[PathBuf],
    fontconfig: impl FnOnce() -> Vec<PathBuf>,
) -> Option<FontSet> {
    let mut seen = HashSet::new();
    let mut fonts = load_fonts(
        collect_candidates(config_path, env_path, Vec::new(), []),
        &mut seen,
    );
    let derived = if fonts.first().is_some_and(has_icons) {
        Vec::new()
    } else {
        fontconfig()
    };
    fonts.extend(load_fonts(
        derived.into_iter().chain(fallbacks.iter().cloned()),
        &mut seen,
    ));
    font_set(fonts)
}

#[cfg(test)]
fn fonts_from_paths(paths: Vec<PathBuf>) -> Option<FontSet> {
    font_set(load_fonts(paths, &mut HashSet::new()))
}

fn font_set(fonts: Vec<FontVec>) -> Option<FontSet> {
    let mut fonts = fonts.into_iter();
    Some(FontSet {
        primary: fonts.next()?,
        fallbacks: fonts.collect(),
    })
}

/// Every parseable font among `paths` not already in `seen`, in order.
fn load_fonts(
    paths: impl IntoIterator<Item = PathBuf>,
    seen: &mut HashSet<PathBuf>,
) -> Vec<FontVec> {
    paths
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .filter_map(|p| FontVec::try_from_vec(std::fs::read(p).ok()?).ok())
        .collect()
}

/// The environment variable that pins the cell font, overriding `fc-match`.
const FONT_ENV: &str = "HYPRBURST_FONT";

/// Common monospace font paths to try when `fc-match` is unavailable.
const FALLBACK_PATHS: &[&str] = &[
    "/usr/share/fonts/TTF/JetBrainsMonoNerdFont-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/liberation/LiberationMono-Regular.ttf",
    "/usr/share/fonts/TTF/Hack-Regular.ttf",
];

/// Load the first *parseable* monospace font from the candidate list, or `None`
/// if nothing resolved. Validation happens here so a readable-but-corrupt file
/// falls through to the next candidate instead of aborting resolution upstream.
pub fn resolve_font(config_path: Option<&str>, family: Option<&str>) -> Option<FontVec> {
    resolve_fonts(config_path, family).map(|fonts| fonts.primary)
}

/// The cache key for fontconfig-derived paths: every input that can change the
/// answer, plus the fontconfig cache directories' `stamps` (mtimes), which
/// `fc-cache` bumps when fonts are installed or removed. Always one line.
fn cache_key(
    config_path: Option<&str>,
    env_path: Option<&str>,
    family: Option<&str>,
    stamps: &[Option<SystemTime>],
) -> String {
    let stamps: Vec<Option<u128>> = stamps
        .iter()
        .map(|t| {
            Some(
                t.as_ref()?
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .ok()?
                    .as_nanos(),
            )
        })
        .collect();
    // `Debug` escapes control characters, so the key never spans lines.
    format!("v1 {:?}", (config_path, env_path, family, stamps))
}

/// Cached fontconfig-derived paths in `file` for `key`, or `lookup()` (written
/// back best-effort) when the cache is missing, stale, corrupt, or names a
/// font that no longer exists.
fn cached_paths(file: &Path, key: &str, lookup: impl FnOnce() -> Vec<PathBuf>) -> Vec<PathBuf> {
    if let Some(paths) = read_cache(file, key) {
        return paths;
    }
    let paths = lookup();
    write_cache(file, key, &paths);
    paths
}

fn read_cache(file: &Path, key: &str) -> Option<Vec<PathBuf>> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut lines = text.lines();
    if lines.next()? != key {
        return None;
    }
    let paths: Vec<PathBuf> = lines.map(PathBuf::from).collect();
    paths.iter().all(|p| p.exists()).then_some(paths)
}

/// Best effort: a failed write only means the next open looks up again.
fn write_cache(file: &Path, key: &str, paths: &[PathBuf]) {
    let mut text = format!("{key}\n");
    for path in paths {
        match path.to_str() {
            Some(p) if !p.contains('\n') => text.push_str(&format!("{p}\n")),
            _ => return,
        }
    }
    let Some(dir) = file.parent() else {
        return;
    };
    // Write-then-rename so a concurrent open never reads a half-written file.
    let tmp = file.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::create_dir_all(dir).is_ok() && std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, file);
    }
}

/// `$XDG_CACHE_HOME`, else `$HOME/.cache`.
fn cache_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".cache")))
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// fontconfig-derived paths through the on-disk cache, when there is a cache
/// directory to use.
fn cached_fontconfig_paths(
    config_path: Option<&str>,
    env_path: Option<&str>,
    family: Option<&str>,
) -> Vec<PathBuf> {
    let Some(home) = cache_home() else {
        return fontconfig_paths(family);
    };
    let stamps = [
        modified(&home.join("fontconfig")),
        modified(Path::new("/var/cache/fontconfig")),
    ];
    let key = cache_key(config_path, env_path, family, &stamps);
    cached_paths(&home.join("hyprburst").join("fonts"), &key, || {
        fontconfig_paths(family)
    })
}

/// fontconfig-derived paths, most specific first: the configured family, then
/// the default monospace / any Nerd Font.
fn fontconfig_paths(family: Option<&str>) -> Vec<PathBuf> {
    family
        .filter(|f| !f.is_empty())
        .and_then(fc_match_family)
        .into_iter()
        .chain(fc_font_candidates())
        .collect()
}

fn collect_candidates(
    config_path: Option<&str>,
    env_path: Option<&str>,
    family_paths: Vec<PathBuf>,
    later: impl IntoIterator<Item = PathBuf>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(p) = config_path.filter(|p| !p.is_empty()) {
        paths.push(PathBuf::from(p));
    }
    if let Some(p) = env_path.filter(|p| !p.is_empty()) {
        paths.push(PathBuf::from(p));
    }
    paths.extend(family_paths);
    paths.extend(later);
    paths
}

/// `fc-match` file for a configured family name. Fontconfig still returns a
/// closest match when the family is missing, so this is a preference, not a
/// guarantee.
fn fc_match_family(family: &str) -> Option<PathBuf> {
    let out = Command::new("fc-match")
        .args(["-f", "%{file}", family])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let file = text.trim();
    if file.is_empty() {
        None
    } else {
        Some(PathBuf::from(file))
    }
}

/// fontconfig-derived candidates. ONE `fc-match monospace` spawn answers both
/// "which file?" and "is it already a Nerd Font?" via a combined format string;
/// only when it isn't do we spend a second spawn on an `fc-list` scan.
fn fc_font_candidates() -> Vec<PathBuf> {
    let Some((default_file, family)) = fc_default_mono() else {
        return Vec::new();
    };
    if family_is_nerd(&family) {
        return vec![default_file];
    }
    let mut candidates = vec![default_file];
    if let Some(nerd) = fc_any_nerd_font() {
        // A dedicated Nerd Font beats the plain default mono (icons vs tofu).
        candidates.insert(0, nerd);
    }
    candidates
}

/// `(file, family)` of the system default monospace, from a single
/// `fc-match -f '%{file}\t%{family}' monospace` invocation.
fn fc_default_mono() -> Option<(PathBuf, String)> {
    let out = Command::new("fc-match")
        .args(["-f", "%{file}\t%{family}", "monospace"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let (file, family) = text.trim().split_once('\t')?;
    let file = file.trim();
    if file.is_empty() {
        return None;
    }
    Some((PathBuf::from(file), family.to_string()))
}

/// Scan all installed fonts for a Nerd Font (`fc-list`), Mono variants preferred.
fn fc_any_nerd_font() -> Option<PathBuf> {
    let out = Command::new("fc-list")
        .arg("-f")
        .arg("%{family}\t%{file}\n")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let list = String::from_utf8(out.stdout).ok()?;
    pick_nerd_font(&list).map(PathBuf::from)
}

/// Does a fontconfig family string name a Nerd Font?
fn family_is_nerd(family: &str) -> bool {
    family.contains("Nerd Font")
}

/// Pick a Nerd Font file from `fc-list -f "%{family}\t%{file}\n"` output,
/// preferring a `Mono` variant (single-cell-wide icons) over any other.
fn pick_nerd_font(list: &str) -> Option<String> {
    let mut first_any: Option<String> = None;
    for line in list.lines() {
        let Some((family, file)) = line.split_once('\t') else {
            continue;
        };
        let file = file.trim();
        if file.is_empty() || !family_is_nerd(family) {
            continue;
        }
        if family.contains("Mono") {
            return Some(file.to_string());
        }
        if first_any.is_none() {
            first_any = Some(file.to_string());
        }
    }
    first_any
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_fonts() -> FontSet {
        FontSet {
            primary: FontVec::try_from_vec(
                include_bytes!("../../tests/fixtures/fonts/primary.ttf").to_vec(),
            )
            .unwrap(),
            fallbacks: vec![
                FontVec::try_from_vec(
                    include_bytes!("../../tests/fixtures/fonts/icons.ttf").to_vec(),
                )
                .unwrap(),
            ],
        }
    }

    #[test]
    fn missing_launcher_icon_uses_fallback_without_replacing_text() {
        let fonts = fixture_fonts();
        let icon = crate::domain::icon::fallback_glyph("firefox", "Firefox")
            .chars()
            .next()
            .unwrap();
        assert_eq!(fonts.primary.glyph_id(icon).0, 0);
        assert_ne!(fonts.fallbacks[0].glyph_id(icon).0, 0);
        assert!(std::ptr::eq(fonts.glyph_font('A').unwrap(), &fonts.primary));
        assert!(std::ptr::eq(
            fonts.glyph_font(icon).expect("fallback icon font"),
            &fonts.fallbacks[0]
        ));
    }

    #[test]
    fn icon_coverage_fits_cell_and_cache_reuses_original_character_at_each_scale() {
        use crate::gpu::grid::{Atlas, CellMetrics, GlyphKey};
        let fonts = fixture_fonts();
        for factor in [1.0, 1.5, 2.0] {
            let (w, h) = ((12.0 * factor) as u32, (20.0 * factor) as u32);
            let mut atlas = Atlas::new((256, 256), CellMetrics::new(w, h));
            let key = GlyphKey::new('\u{f269}');
            let slot = atlas.get_or_insert(key).unwrap();
            assert!(slot.newly_inserted);
            let pixels = fonts.coverage(
                '\u{f269}',
                (w, h),
                PxScale::from(20.0 * factor),
                16.0 * factor,
            );
            let points: Vec<_> = pixels
                .iter()
                .enumerate()
                .filter(|(_, c)| **c > 0)
                .map(|(i, _)| (i as u32 % w, i as u32 / w))
                .collect();
            assert!(!points.is_empty(), "icon must be visible");
            let left = points.iter().map(|p| p.0).min().unwrap();
            let right = points.iter().map(|p| p.0).max().unwrap();
            let top = points.iter().map(|p| p.1).min().unwrap();
            let bottom = points.iter().map(|p| p.1).max().unwrap();
            assert!(
                left > 0 && right < w - 1 && top > 0 && bottom < h - 1,
                "icon must fit without clipping"
            );
            assert!((left as i32 - (w - 1 - right) as i32).abs() <= 1);
            assert!((top as i32 - (h - 1 - bottom) as i32).abs() <= 1);
            let cached = atlas.get_or_insert(key).unwrap();
            assert!(!cached.newly_inserted);
            assert_eq!(cached.px, slot.px);
            assert_ne!(atlas.get_or_insert(GlyphKey::new('A')).unwrap().px, slot.px);
        }
    }

    #[test]
    fn no_icon_font_draws_deliberate_generic_symbol_not_tofu() {
        let mut fonts = fixture_fonts();
        fonts.fallbacks.clear();
        for icon in ['\u{f269}', '\u{f120}', '\u{f07b}', '\u{f0001}'] {
            assert!(
                fonts
                    .coverage(icon, (12, 20), PxScale::from(20.0), 16.0)
                    .iter()
                    .any(|c| *c > 0)
            );
        }
        assert!(
            fonts
                .coverage(' ', (12, 20), PxScale::from(20.0), 16.0)
                .iter()
                .all(|c| *c == 0)
        );
    }

    /// Ink bounding box `(left, right, top, bottom)` of a `w`-wide coverage map.
    fn ink_bounds(pixels: &[u8], w: u32) -> (u32, u32, u32, u32) {
        let points: Vec<_> = pixels
            .iter()
            .enumerate()
            .filter(|(_, c)| **c > 0)
            .map(|(i, _)| (i as u32 % w, i as u32 / w))
            .collect();
        assert!(!points.is_empty(), "glyph must be visible");
        (
            points.iter().map(|p| p.0).min().unwrap(),
            points.iter().map(|p| p.0).max().unwrap(),
            points.iter().map(|p| p.1).min().unwrap(),
            points.iter().map(|p| p.1).max().unwrap(),
        )
    }

    /// Join a left and a right `w`×`h` tile into one `2w`×`h` coverage map.
    fn join_halves(left: &[u8], right: &[u8], w: u32, h: u32) -> Vec<u8> {
        let w = w as usize;
        let mut joined = Vec::with_capacity(2 * w * h as usize);
        for row in 0..h as usize {
            joined.extend_from_slice(&left[row * w..(row + 1) * w]);
            joined.extend_from_slice(&right[row * w..(row + 1) * w]);
        }
        joined
    }

    #[test]
    fn two_cell_icon_is_larger_than_one_cell_fit_and_never_clipped() {
        let fonts = fixture_fonts();
        let (w, h, scale, ascent) = (12, 25, PxScale::from(25.0), 20.0);
        let icon = '\u{f269}';
        let single = fonts.coverage(icon, (w, h), scale, ascent);
        let left = fonts.coverage_span(icon, GlyphSpan::Left, (w, h), scale, ascent);
        let right = fonts.coverage_span(icon, GlyphSpan::Right, (w, h), scale, ascent);
        assert_eq!(left.len(), (w * h) as usize);
        assert_eq!(right.len(), (w * h) as usize);
        let wide = join_halves(&left, &right, w, h);

        let (sl, sr, st, sb) = ink_bounds(&single, w);
        let (wl, wr, wt, wb) = ink_bounds(&wide, 2 * w);
        assert!(wr - wl > sr - sl, "two cells give the icon more width");
        assert!(wb - wt >= sb - st, "and never less height");
        assert!(
            wl > 0 && wr < 2 * w - 1 && wt > 0 && wb < h - 1,
            "wide icon must fit both cells without clipping"
        );
        assert!((wl as i32 - (2 * w - 1 - wr) as i32).abs() <= 1, "centred");
        assert!(left.iter().any(|c| *c > 0) && right.iter().any(|c| *c > 0));
    }

    #[test]
    fn text_and_badge_ignore_the_two_cell_span() {
        let fonts = fixture_fonts();
        let (size, scale) = ((12, 20), PxScale::from(20.0));
        // Non-icons are never split: the left half is the one-cell glyph.
        assert_eq!(
            fonts.coverage_span('A', GlyphSpan::Left, size, scale, 16.0),
            fonts.coverage('A', size, scale, 16.0)
        );
        // Without an icon font, the square badge stays in the left cell.
        let mut fonts = fixture_fonts();
        fonts.fallbacks.clear();
        let icon = '\u{f269}';
        assert_eq!(
            fonts.coverage_span(icon, GlyphSpan::Left, size, scale, 16.0),
            fonts.coverage(icon, size, scale, 16.0)
        );
        assert!(
            fonts
                .coverage_span(icon, GlyphSpan::Right, size, scale, 16.0)
                .iter()
                .all(|c| *c == 0)
        );
    }

    #[test]
    fn small_icon_outlines_are_enlarged_to_remain_legible() {
        let fonts = fixture_fonts();
        let pixels = fonts.coverage('\u{f269}', (12, 20), PxScale::from(1.0), 16.0);
        let columns: Vec<_> = pixels
            .iter()
            .enumerate()
            .filter(|(_, c)| **c > 0)
            .map(|(i, _)| i % 12)
            .collect();
        assert!(columns.iter().max().unwrap() - columns.iter().min().unwrap() >= 7);
    }

    #[test]
    fn loading_keeps_first_parseable_font_and_deduplicates_fallbacks() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let primary = root.join("tests/fixtures/fonts/primary.ttf");
        let icons = root.join("tests/fixtures/fonts/icons.ttf");
        let fonts = fonts_from_paths(vec![
            root.join("no-such-font.ttf"),
            root.join("Cargo.toml"),
            primary.clone(),
            icons.clone(),
            primary,
            icons,
        ])
        .unwrap();
        assert_ne!(fonts.primary.glyph_id('M').0, 0);
        assert_eq!(fonts.primary.glyph_id('\u{f269}').0, 0);
        assert_eq!(fonts.fallbacks.len(), 1);
        assert_ne!(
            fonts.glyph_font('\u{f269}').unwrap().glyph_id('\u{f269}').0,
            0
        );
    }

    #[test]
    fn primary_icons_win_and_fallbacks_do_not_change_text_coverage() {
        let mut fonts = fixture_fonts();
        let text = fonts.coverage('A', (12, 20), PxScale::from(20.0), 16.0);
        assert!(text.iter().any(|c| *c > 0));
        fonts.fallbacks.clear();
        assert_eq!(
            fonts.coverage('A', (12, 20), PxScale::from(20.0), 16.0),
            text
        );
        let mut fonts = fixture_fonts();
        std::mem::swap(&mut fonts.primary, &mut fonts.fallbacks[0]);
        assert!(std::ptr::eq(
            fonts.glyph_font('\u{f269}').unwrap(),
            &fonts.primary
        ));
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fonts")
            .join(name)
    }

    #[test]
    fn has_icons_detects_the_nerd_font_probe_glyph() {
        let fonts = fixture_fonts();
        assert!(!has_icons(&fonts.primary));
        assert!(has_icons(&fonts.fallbacks[0]));
    }

    #[test]
    fn configured_icon_font_skips_fontconfig() {
        for (config, env) in [
            (Some(fixture("icons.ttf")), None),
            (None, Some(fixture("icons.ttf"))),
        ] {
            let fonts = resolve_with(
                config.as_deref().and_then(|p| p.to_str()),
                env.as_deref().and_then(|p| p.to_str()),
                &[fixture("primary.ttf")],
                || panic!("fontconfig must not be consulted"),
            )
            .unwrap();
            assert!(has_icons(&fonts.primary));
            assert_eq!(fonts.fallbacks.len(), 1);
            assert_ne!(fonts.fallbacks[0].glyph_id('M').0, 0);
        }
    }

    #[test]
    fn configured_text_font_still_gets_fontconfig_icons() {
        let mut called = false;
        let fonts = resolve_with(fixture("primary.ttf").to_str(), None, &[], || {
            called = true;
            vec![fixture("icons.ttf")]
        })
        .unwrap();
        assert!(called);
        assert!(!has_icons(&fonts.primary));
        assert!(has_icons(&fonts.fallbacks[0]));
    }

    #[test]
    fn unparseable_or_missing_config_falls_through_to_fontconfig() {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        for config in [manifest.to_str(), None] {
            let mut called = false;
            let fonts = resolve_with(config, None, &[fixture("icons.ttf")], || {
                called = true;
                vec![fixture("primary.ttf")]
            })
            .unwrap();
            assert!(called);
            assert_ne!(fonts.primary.glyph_id('M').0, 0);
            assert!(!has_icons(&fonts.primary));
            assert_eq!(fonts.fallbacks.len(), 1);
        }
    }

    fn cache_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hyprburst-font-cache-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("hyprburst").join("fonts")
    }

    #[test]
    fn cache_miss_runs_lookup_and_a_hit_reuses_it() {
        let file = cache_file("hit");
        let fonts = vec![fixture("icons.ttf"), fixture("primary.ttf")];
        let mut calls = 0;
        assert_eq!(
            cached_paths(&file, "k", || {
                calls += 1;
                fonts.clone()
            }),
            fonts
        );
        assert_eq!(calls, 1);
        assert_eq!(
            cached_paths(&file, "k", || panic!("cache hit must not look up")),
            fonts
        );
    }

    #[test]
    fn changed_key_reruns_lookup() {
        let file = cache_file("key");
        cached_paths(&file, "old", || vec![fixture("icons.ttf")]);
        assert_eq!(
            cached_paths(&file, "new", || vec![fixture("primary.ttf")]),
            vec![fixture("primary.ttf")]
        );
        assert_eq!(
            cached_paths(&file, "new", || panic!("rewritten cache must hit")),
            vec![fixture("primary.ttf")]
        );
    }

    #[test]
    fn vanished_cached_font_reruns_lookup() {
        let file = cache_file("gone");
        let gone = file.with_file_name("gone.ttf");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&gone, b"x").unwrap();
        cached_paths(&file, "k", || vec![gone.clone()]);
        std::fs::remove_file(&gone).unwrap();
        let mut called = false;
        cached_paths(&file, "k", || {
            called = true;
            vec![fixture("icons.ttf")]
        });
        assert!(called);
    }

    #[test]
    fn empty_or_corrupt_cache_reruns_lookup() {
        for contents in ["", "not the key\n/some/font.ttf\n"] {
            let file = cache_file("corrupt");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, contents).unwrap();
            let mut called = false;
            cached_paths(&file, "k", || {
                called = true;
                Vec::new()
            });
            assert!(called, "{contents:?}");
        }
    }

    #[test]
    fn cache_key_tracks_every_input_on_one_line() {
        let t = Some(SystemTime::UNIX_EPOCH);
        let later = Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1));
        let base = cache_key(Some("/c.ttf"), Some("/e.ttf"), Some("Mono"), &[t, None]);
        for other in [
            cache_key(None, Some("/e.ttf"), Some("Mono"), &[t, None]),
            cache_key(Some("/c.ttf"), None, Some("Mono"), &[t, None]),
            cache_key(Some("/c.ttf"), Some("/e.ttf"), Some("Sans"), &[t, None]),
            cache_key(Some("/c.ttf"), Some("/e.ttf"), Some("Mono"), &[later, None]),
            cache_key(Some("/c.ttf"), Some("/e.ttf"), Some("Mono"), &[t, t]),
        ] {
            assert_ne!(base, other);
        }
        assert_eq!(
            base,
            cache_key(Some("/c.ttf"), Some("/e.ttf"), Some("Mono"), &[t, None])
        );
        assert!(!cache_key(Some("a\nb"), None, None, &[]).contains('\n'));
    }

    #[test]
    fn family_is_nerd_detects_nerd_fonts() {
        assert!(family_is_nerd("JetBrainsMono Nerd Font Mono"));
        assert!(family_is_nerd("Hack Nerd Font"));
        assert!(!family_is_nerd("DejaVu Sans Mono"));
        assert!(!family_is_nerd("monospace"));
    }

    #[test]
    fn pick_nerd_font_prefers_mono_variant() {
        let list = "\
DejaVu Sans Mono\t/usr/share/fonts/dejavu.ttf
Hack Nerd Font\t/usr/share/fonts/hack-nf.ttf
JetBrainsMono Nerd Font Mono\t/usr/share/fonts/jbmono-nfm.ttf
";
        assert_eq!(
            pick_nerd_font(list).as_deref(),
            Some("/usr/share/fonts/jbmono-nfm.ttf"),
            "a Mono Nerd Font should win over a non-Mono one",
        );
    }

    #[test]
    fn pick_nerd_font_falls_back_to_any_nerd_font() {
        let list = "\
DejaVu Sans Mono\t/usr/share/fonts/dejavu.ttf
Symbols Nerd Font\t/usr/share/fonts/symbols-nf.ttf
";
        assert_eq!(
            pick_nerd_font(list).as_deref(),
            Some("/usr/share/fonts/symbols-nf.ttf"),
        );
    }

    #[test]
    fn pick_nerd_font_returns_none_without_a_nerd_font() {
        let list = "\
DejaVu Sans Mono\t/usr/share/fonts/dejavu.ttf
Liberation Mono\t/usr/share/fonts/liberation.ttf
";
        assert_eq!(pick_nerd_font(list), None);
    }

    #[test]
    fn pick_nerd_font_skips_malformed_and_empty_lines() {
        let list = "no-tab-here\n\nHack Nerd Font\t\nFiraCode Nerd Font Mono\t/f.ttf\n";
        // The empty-file Hack line is skipped; the Mono one wins.
        assert_eq!(pick_nerd_font(list).as_deref(), Some("/f.ttf"));
    }

    #[test]
    fn family_comes_after_path_and_env_before_fontconfig() {
        let paths = collect_candidates(
            Some("/cfg.ttf"),
            Some("/env.ttf"),
            vec![PathBuf::from("/family.ttf")],
            [PathBuf::from("/fc.ttf"), PathBuf::from("/fallback.ttf")],
        );
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/cfg.ttf"),
                PathBuf::from("/env.ttf"),
                PathBuf::from("/family.ttf"),
                PathBuf::from("/fc.ttf"),
                PathBuf::from("/fallback.ttf"),
            ]
        );
    }

    #[test]
    fn empty_path_and_env_skip_to_family() {
        let paths = collect_candidates(
            Some(""),
            Some(""),
            vec![PathBuf::from("/family.ttf")],
            [PathBuf::from("/fc.ttf")],
        );
        assert_eq!(
            paths,
            vec![PathBuf::from("/family.ttf"), PathBuf::from("/fc.ttf")]
        );
    }
}

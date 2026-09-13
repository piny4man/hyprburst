//! Monospace-font resolution for the GUI launcher.
//!
//! Both GPU frontends retain the requested primary text font and search fallback
//! fonts for missing glyphs. Private-use icons are fitted to the primary cell;
//! without an icon font, a procedural four-tile app symbol avoids missing-glyph
//! boxes. The `tui` fallback still uses the hosting terminal's font.
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
//! Cold-start matters here: the common case (a Nerd Font as the default
//! monospace) costs exactly one `fc-match` spawn; the scan-for-any-Nerd-Font
//! path adds one `fc-list`.

use std::path::PathBuf;
use std::process::Command;

use ab_glyph::{Font, FontVec, PxScale};

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

    pub(crate) fn coverage(
        &self,
        ch: char,
        size: (u32, u32),
        scale: PxScale,
        ascent: f32,
    ) -> Vec<u8> {
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
fn is_icon(ch: char) -> bool {
    matches!(ch as u32, 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd)
}

/// Resolve once per window. Keeping the font set immutable means atlas keys can
/// remain characters: a given character always selects the same face and glyph.
pub(crate) fn resolve_fonts(config_path: Option<&str>, family: Option<&str>) -> Option<FontSet> {
    fonts_from_paths(candidate_paths(config_path, family))
}

fn fonts_from_paths(paths: Vec<PathBuf>) -> Option<FontSet> {
    let mut seen = std::collections::HashSet::new();
    let mut fonts = paths
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .filter_map(|p| FontVec::try_from_vec(std::fs::read(p).ok()?).ok());
    Some(FontSet {
        primary: fonts.next()?,
        fallbacks: fonts.collect(),
    })
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
    candidate_paths(config_path, family)
        .into_iter()
        .find_map(|p| {
            let bytes = std::fs::read(&p).ok()?;
            FontVec::try_from_vec(bytes).ok()
        })
}

/// The ordered list of font paths to try, most-specific first.
fn candidate_paths(config_path: Option<&str>, family: Option<&str>) -> Vec<PathBuf> {
    let env_path = std::env::var(FONT_ENV).ok();
    let family_paths = family
        .filter(|f| !f.is_empty())
        .and_then(fc_match_family)
        .into_iter()
        .collect();
    collect_candidates(
        config_path,
        env_path.as_deref(),
        family_paths,
        fc_font_candidates()
            .into_iter()
            .chain(FALLBACK_PATHS.iter().map(PathBuf::from)),
    )
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

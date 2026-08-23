//! Monospace-font resolution for the GUI launcher.
//!
//! The windowed launcher rasterizes its own glyphs from a *single* font, so that
//! font must carry both the text glyphs and the launcher's Nerd Font icon glyphs.
//! (The `tui` fallback gets icons from the *hosting terminal's* font instead.)
//! Resolution order, most specific first:
//!
//! 1. an explicit path from `[font] path` in the config,
//! 2. the `HYPRBURST_FONT` environment variable (a `.ttf`/`.otf` path),
//! 3. fontconfig candidates: the default monospace if it is already a Nerd Font,
//!    otherwise any installed Nerd Font (a `Mono` variant preferred), then the
//!    plain default monospace (may lack icon glyphs),
//! 4. a few common hard-coded paths, as a last resort.
//!
//! Cold-start matters here: the common case (a Nerd Font as the default
//! monospace) costs exactly one `fc-match` spawn; the scan-for-any-Nerd-Font
//! path adds one `fc-list`.

use std::path::PathBuf;
use std::process::Command;

use ab_glyph::FontVec;

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
pub fn resolve_font(config_path: Option<&str>) -> Option<FontVec> {
    candidate_paths(config_path).into_iter().find_map(|p| {
        let bytes = std::fs::read(&p).ok()?;
        FontVec::try_from_vec(bytes).ok()
    })
}

/// The ordered list of font paths to try, most-specific first.
fn candidate_paths(config_path: Option<&str>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(p) = config_path.filter(|p| !p.is_empty()) {
        paths.push(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var(FONT_ENV)
        && !p.is_empty()
    {
        paths.push(PathBuf::from(p));
    }
    paths.extend(fc_font_candidates());
    paths.extend(FALLBACK_PATHS.iter().map(PathBuf::from));
    paths
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
}

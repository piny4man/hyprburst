//! Monospace-font resolution for the GUI launcher.
//!
//! The windowed launcher rasterizes its own glyphs from a *single* font, so that
//! font must carry both the text glyphs and the launcher's Nerd Font icon glyphs.
//! (The `tui` fallback gets icons from the *hosting terminal's* font instead.)
//! Resolution order, most specific first:
//!
//! 1. an explicit path from `[font] path` in the config,
//! 2. the `HYPRBURST_FONT` environment variable (a `.ttf`/`.otf` path),
//! 3. a **Nerd Font** discovered via fontconfig — the default monospace if it is
//!    already a Nerd Font, otherwise any installed Nerd Font (a `Mono` variant
//!    preferred), so icons render instead of tofu out of the box,
//! 4. the system's default monospace via `fc-match` (may lack icon glyphs),
//! 5. a few common hard-coded paths, as a last resort.

use std::path::PathBuf;
use std::process::Command;

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

/// Read the bytes of a usable monospace font, or `None` if nothing resolved.
/// `config_path` is the optional `[font] path` from the config — tried first.
pub fn resolve_font_bytes(config_path: Option<&str>) -> Option<Vec<u8>> {
    candidate_paths(config_path)
        .into_iter()
        .find_map(|p| std::fs::read(&p).ok())
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
    // Prefer a Nerd Font so the launcher's icon glyphs render; fall back to the
    // plain default monospace (icons may show as tofu — set `[font] path`).
    if let Some(p) = fc_nerd_font() {
        paths.push(p);
    }
    if let Some(p) = fc_match("monospace") {
        paths.push(p);
    }
    paths.extend(FALLBACK_PATHS.iter().map(PathBuf::from));
    paths
}

/// Resolve a fontconfig pattern to a concrete font file via `fc-match`. `None`
/// if `fc-match` is missing or returns nothing.
fn fc_match(pattern: &str) -> Option<PathBuf> {
    let path = fc_match_field(pattern, "%{file}")?;
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

/// Find an installed Nerd Font that carries both text and icon glyphs: the
/// default monospace if it already is one, otherwise any Nerd Font from
/// `fc-list` (a `Mono` variant preferred).
fn fc_nerd_font() -> Option<PathBuf> {
    // 1. The default monospace is already a Nerd Font — keep it.
    if let Some(family) = fc_match_field("monospace", "%{family}")
        && family_is_nerd(&family)
        && let Some(file) = fc_match_field("monospace", "%{file}")
        && !file.is_empty()
    {
        return Some(PathBuf::from(file));
    }
    // 2. Otherwise scan all installed fonts for a Nerd Font.
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

/// Run `fc-match -f <format> <pattern>` and return its trimmed stdout.
fn fc_match_field(pattern: &str, format: &str) -> Option<String> {
    let out = Command::new("fc-match")
        .arg("-f")
        .arg(format)
        .arg(pattern)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8(out.stdout).ok()?.trim().to_string())
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

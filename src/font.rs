//! Monospace-font resolution for the GUI launcher.
//!
//! The windowed launcher rasterizes its own glyphs, so it needs a monospace font
//! that carries the launcher's Nerd Font icon glyphs. (The `tui` fallback gets
//! icons from the *hosting terminal's* font instead.) Resolution order, most
//! specific first:
//!
//! 1. an explicit path from `[font] path` in the config,
//! 2. the `HYPRBURST_FONT` environment variable (a `.ttf`/`.otf` path),
//! 3. the system's default monospace via `fc-match` — usually the user's Nerd
//!    Font,
//! 4. a few common hard-coded paths, as a last resort (these likely lack icon
//!    glyphs, so icons fall back to tofu — set `[font] path` to fix that).

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
    if let Some(p) = fc_match("monospace") {
        paths.push(p);
    }
    paths.extend(FALLBACK_PATHS.iter().map(PathBuf::from));
    paths
}

/// Resolve a fontconfig pattern to a concrete font file via `fc-match`. `None`
/// if `fc-match` is missing or returns nothing.
fn fc_match(pattern: &str) -> Option<PathBuf> {
    let out = Command::new("fc-match")
        .arg("-f")
        .arg("%{file}")
        .arg(pattern)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8(out.stdout).ok()?;
    let path = path.trim();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

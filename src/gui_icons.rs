//! Themed-icon support and headless cost model (Phase 6 of the Freya bake-off).
//!
//! Phase 6 is the *measured bonus*: render real themed app icons in the
//! native-GUI POC and quantify what they cost versus the Nerd Font glyph path,
//! without letting an image regression fail the bake-off. The live render lives
//! in [`crate::gui`] (an `ImageViewer` per entry, Skia-decoded). This module
//! holds the parts that need no Freya runtime, so they can be unit-tested and
//! driven by the benchmark harness:
//!
//! - [`decode_rgba`] — decode encoded image bytes to RGBA pixels, the work
//!   Freya's `ImageViewer` performs on Skia. Modeled here with the `image` crate
//!   (already in the `freya-spike` tree) so the cost is *real* rather than
//!   invented; absolute numbers differ by decoder, but the character of the work
//!   — entropy-decode a PNG into a pixel buffer — is the same.
//! - [`IconRenderModel`] — the themed analog of the glyph [`crate::gui::build_frame`]
//!   paint closure: each frame it builds the same glyph frame (the shared base
//!   work) **and**, for every newly-visible entry, decodes its icon once into a
//!   cache. So the harness column captures the added per-frame decode on cache
//!   misses (cold start, and when typing reveals not-yet-seen apps) plus the
//!   retained-pixel memory — the exact signature of a real texture cache. GPU
//!   upload/compositing is excluded, as in every other column.
//!
//! The benchmark icons are deterministic synthetic PNGs ([`synthetic_icon_png`]),
//! not live theme files, so runs stay comparable across machines and time per the
//! bake-off's fixed-synthetic-input rule. Resolving a real theme path is cheap
//! relative to decode and varies by machine, so it is intentionally excluded from
//! the modeled cost (the live `--measure` runs capture the true process RSS).

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use image::ExtendedColorType;
use image::ImageEncoder;
use image::codecs::png::PngEncoder;

use crate::config::Config;
use crate::desktop::DesktopEntry;
use crate::gui::{IconMode, build_frame};
use crate::launcher_core::LauncherCore;

/// Square side, in pixels, of a synthetic benchmark icon. Sized in the range of
/// real themed app icons (32–48px) so the decode cost is representative.
pub const ICON_SIZE: u32 = 48;

/// A decoded icon: RGBA pixels and their dimensions — what a texture cache holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl LoadedIcon {
    /// Bytes of decoded pixel data retained in the cache for this icon.
    pub fn byte_len(&self) -> usize {
        self.rgba.len()
    }
}

/// Decode encoded image bytes (PNG here) into RGBA pixels — the per-icon decode
/// work the themed path pays on a cache miss. Returns `None` if the bytes don't
/// decode, which is exactly when the live render falls back to the glyph.
pub fn decode_rgba(bytes: &[u8]) -> Option<LoadedIcon> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(LoadedIcon {
        width,
        height,
        rgba: rgba.into_raw(),
    })
}

/// Hash an icon name to a stable per-icon seed, so each synthetic icon differs
/// (and thus compresses/decodes with realistic, varied cost) while staying
/// reproducible across runs.
fn seed_for(name: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    hasher.finish()
}

/// Deterministic high-entropy RGBA pixels for a synthetic icon. High entropy
/// keeps the encoded PNG from collapsing to almost nothing (which would
/// understate decode cost) and varies per `seed` so different icons aren't
/// identical.
fn synthetic_icon_pixels(seed: u64, size: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let h = seed
                .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                .wrapping_add((x as u64) << 32)
                .wrapping_add((y as u64) << 16)
                .wrapping_add(
                    (x as u64)
                        .wrapping_mul(y as u64)
                        .wrapping_mul(2_654_435_761),
                );
            px.push((h & 0xFF) as u8);
            px.push(((h >> 8) & 0xFF) as u8);
            px.push(((h >> 16) & 0xFF) as u8);
            px.push(0xFF);
        }
    }
    px
}

/// Encode a deterministic synthetic PNG for the given `seed` at [`ICON_SIZE`].
/// Used to feed the headless decode model real encoded bytes without depending
/// on a machine's icon theme.
pub fn synthetic_icon_png(seed: u64) -> Vec<u8> {
    let pixels = synthetic_icon_pixels(seed, ICON_SIZE);
    let mut buf = Vec::new();
    PngEncoder::new(&mut buf)
        .write_image(&pixels, ICON_SIZE, ICON_SIZE, ExtendedColorType::Rgba8)
        .expect("encoding a synthetic RGBA icon to PNG never fails");
    buf
}

/// Headless model of the themed-icon variant's per-frame work: the shared glyph
/// frame build plus a decode-on-cache-miss for each visible entry's icon. The
/// analog of [`crate::gui::build_frame`] used as a paint closure, but for the
/// image path — see the module docs.
pub struct IconRenderModel {
    config: Config,
    /// Encoded PNG bytes per icon name, generated up front so the measured frame
    /// pays only resolution (a cheap map lookup) + decode, mirroring a real
    /// frontend that reads the file once and decodes on demand.
    sources: HashMap<String, Vec<u8>>,
    /// Decoded icons retained by name — the texture cache.
    cache: HashMap<String, LoadedIcon>,
    /// Cumulative number of decodes performed (cache misses), across all frames.
    decoded_count: u64,
}

impl IconRenderModel {
    /// Build a model for `apps`, pre-generating one synthetic PNG per distinct
    /// icon name.
    pub fn new(config: Config, apps: &[DesktopEntry]) -> Self {
        let mut sources = HashMap::new();
        for app in apps {
            sources
                .entry(app.icon.clone())
                .or_insert_with(|| synthetic_icon_png(seed_for(&app.icon)));
        }
        Self {
            config,
            sources,
            cache: HashMap::new(),
            decoded_count: 0,
        }
    }

    /// One frame: build the shared glyph frame (the base work every variant
    /// does), then decode any visible icon not yet cached. Cache hits are a cheap
    /// lookup; misses pay the decode — exactly the themed path's added cost.
    pub fn paint(&mut self, core: &mut LauncherCore) {
        let view = core.view();
        let frame = build_frame(&view, &self.config, IconMode::Glyph);
        std::hint::black_box(&frame);
        for entry in &view.entries {
            self.ensure_decoded(entry.icon_name);
        }
    }

    /// Decode `name`'s icon into the cache if absent. No-op on a hit.
    fn ensure_decoded(&mut self, name: &str) {
        if self.cache.contains_key(name) {
            return;
        }
        if let Some(bytes) = self.sources.get(name)
            && let Some(icon) = decode_rgba(bytes)
        {
            self.cache.insert(name.to_string(), icon);
            self.decoded_count += 1;
        }
    }

    /// Decode every known icon into the cache — the steady state after every app
    /// has been seen once. Used to report total retained texture memory.
    pub fn decode_all(&mut self) {
        let names: Vec<String> = self.sources.keys().cloned().collect();
        for name in names {
            self.ensure_decoded(&name);
        }
    }

    /// Cumulative decodes performed so far (cache misses).
    pub fn decoded_count(&self) -> u64 {
        self.decoded_count
    }

    /// Total bytes of decoded pixel data currently retained in the cache.
    pub fn cache_bytes(&self) -> usize {
        self.cache.values().map(LoadedIcon::byte_len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::synthetic_apps;

    #[test]
    fn synthetic_png_round_trips_to_expected_dimensions() {
        let png = synthetic_icon_png(seed_for("firefox"));
        // A real PNG starts with the 8-byte signature.
        assert_eq!(
            &png[..8],
            &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n']
        );
        let icon = decode_rgba(&png).expect("synthetic PNG must decode");
        assert_eq!((icon.width, icon.height), (ICON_SIZE, ICON_SIZE));
        assert_eq!(icon.rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
    }

    #[test]
    fn synthetic_png_is_deterministic_and_varies_by_name() {
        // Same name → identical bytes (reproducible across runs/machines).
        assert_eq!(
            synthetic_icon_png(seed_for("firefox")),
            synthetic_icon_png(seed_for("firefox")),
        );
        // Different names → different icons.
        assert_ne!(
            synthetic_icon_png(seed_for("firefox")),
            synthetic_icon_png(seed_for("kitty")),
        );
    }

    #[test]
    fn decode_rgba_rejects_garbage_bytes() {
        assert!(decode_rgba(b"not an image").is_none());
    }

    #[test]
    fn paint_decodes_each_visible_icon_once_then_caches() {
        let apps = synthetic_apps();
        let mut model = IconRenderModel::new(Config::default(), &apps);
        let mut core = LauncherCore::from_apps(apps, Config::default());

        model.paint(&mut core);
        let after_first = model.decoded_count();
        assert!(
            after_first > 0,
            "first frame should decode the initially-visible icons",
        );

        // A second identical frame must not re-decode anything — pure cache hits.
        model.paint(&mut core);
        assert_eq!(
            model.decoded_count(),
            after_first,
            "cached icons must not be decoded again",
        );
    }

    #[test]
    fn decode_all_retains_pixels_for_every_distinct_icon() {
        let apps = synthetic_apps();
        let distinct: std::collections::BTreeSet<&str> =
            apps.iter().map(|a| a.icon.as_str()).collect();
        let mut model = IconRenderModel::new(Config::default(), &apps);

        model.decode_all();

        assert_eq!(model.decoded_count(), distinct.len() as u64);
        assert_eq!(
            model.cache_bytes(),
            distinct.len() * (ICON_SIZE * ICON_SIZE * 4) as usize,
        );
    }
}

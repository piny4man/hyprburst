//! Golden-file tests for Phase 5 of the publish-readiness plan: AUR packaging
//! preparation.
//!
//! These tests pin the Arch packaging artifacts so they cannot silently drift:
//!   * `packaging/aur/PKGBUILD` builds the `hyprburst` package from the crates.io
//!     source tarball, declares the right metadata/deps, and installs the binary,
//!     the drop-in Hyprland config, and the license.
//!   * `pkgver` stays in lockstep across `Cargo.toml`, the `PKGBUILD`, and the
//!     generated `.SRCINFO` — bumping the crate version without regenerating the
//!     AUR files fails CI.
//!   * `packaging/aur/README.md` documents how to publish and update the package.

use std::path::PathBuf;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let path = manifest_dir().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

/// Parse `version = "x.y.z"` from the `[package]` section of Cargo.toml.
fn crate_version() -> String {
    let cargo = read("Cargo.toml");
    cargo
        .lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("version")
                .and_then(|rest| rest.trim_start().strip_prefix('='))
                .map(|rest| rest.trim().trim_matches('"').to_string())
        })
        .expect("Cargo.toml should declare a package version")
}

/// Parse a bare `key=value` assignment (no surrounding spaces, AUR style) from
/// the top level of the PKGBUILD.
fn pkgbuild_field(pkgbuild: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    pkgbuild.lines().find_map(|line| {
        line.strip_prefix(&prefix).map(|v| {
            v.trim()
                .trim_matches(|c| c == '(' || c == ')')
                .trim_matches('\'')
                .trim_matches('"')
                .to_string()
        })
    })
}

#[test]
fn pkgbuild_has_core_metadata() {
    let pkgbuild = read("packaging/aur/PKGBUILD");
    assert_eq!(
        pkgbuild_field(&pkgbuild, "pkgname").as_deref(),
        Some("hyprburst"),
        "PKGBUILD pkgname should be `hyprburst` (the confirmed-available crate/AUR name)"
    );
    assert_eq!(
        pkgbuild_field(&pkgbuild, "license").as_deref(),
        Some("GPL-3.0-or-later"),
        "PKGBUILD license should match the project license"
    );
    assert!(
        pkgbuild_field(&pkgbuild, "url").is_some_and(|u| u.contains("piny4man/hyprburst")),
        "PKGBUILD url should point at the hyprburst repo"
    );
    assert!(
        pkgbuild_field(&pkgbuild, "pkgrel").is_some(),
        "PKGBUILD should declare a pkgrel"
    );
}

#[test]
fn pkgbuild_version_matches_crate_version() {
    let pkgbuild = read("packaging/aur/PKGBUILD");
    assert_eq!(
        pkgbuild_field(&pkgbuild, "pkgver").as_deref(),
        Some(crate_version().as_str()),
        "PKGBUILD pkgver must match the Cargo.toml crate version — bump them together"
    );
}

#[test]
fn srcinfo_is_consistent_with_pkgbuild() {
    let srcinfo = read("packaging/aur/.SRCINFO");
    let version = crate_version();
    assert!(
        srcinfo.contains("pkgbase = hyprburst") && srcinfo.contains("pkgname = hyprburst"),
        ".SRCINFO should describe the `hyprburst` package"
    );
    assert!(
        srcinfo.contains(&format!("pkgver = {version}")),
        ".SRCINFO pkgver must match the crate version {version} — regenerate it with `makepkg --printsrcinfo`"
    );
}

#[test]
fn pkgbuild_builds_from_crates_io_tarball() {
    let pkgbuild = read("packaging/aur/PKGBUILD");
    assert!(
        pkgbuild.contains("static.crates.io/crates/$pkgname/$pkgname-$pkgver.crate"),
        "PKGBUILD should fetch the source from the crates.io release tarball"
    );
    assert!(
        pkgbuild.contains("cargo build --frozen --release"),
        "PKGBUILD should build a release binary against the locked dependencies"
    );
}

#[test]
fn pkgbuild_declares_runtime_and_build_deps() {
    let pkgbuild = read("packaging/aur/PKGBUILD");
    for dep in ["gcc-libs", "glibc"] {
        assert!(
            pkgbuild.contains(dep),
            "PKGBUILD depends should include `{dep}` for the Rust binary"
        );
    }
    assert!(
        pkgbuild.contains("makedepends=('cargo')"),
        "PKGBUILD should build-depend on cargo"
    );
    assert!(
        pkgbuild.contains("optdepends=") && pkgbuild.contains("hyprland:"),
        "PKGBUILD should list hyprland (and a terminal/font) as optional Arch/Hyprland runtime deps"
    );
}

#[test]
fn pkgbuild_installs_binary_conf_and_license() {
    let pkgbuild = read("packaging/aur/PKGBUILD");
    assert!(
        pkgbuild.contains("\"$pkgdir/usr/bin/$pkgname\""),
        "PKGBUILD should install the hyprburst binary into /usr/bin"
    );
    assert!(
        pkgbuild.contains("packaging/hyprburst.conf"),
        "PKGBUILD should ship the drop-in Hyprland config (packaging/hyprburst.conf)"
    );
    assert!(
        pkgbuild.contains("/usr/share/licenses/$pkgname/LICENSE"),
        "PKGBUILD should install the GPL license into /usr/share/licenses"
    );
}

#[test]
fn aur_readme_documents_publish_and_update_steps() {
    let doc = read("packaging/aur/README.md");
    let lower = doc.to_lowercase();
    for needle in ["makepkg", ".srcinfo", "aur.archlinux.org", "pkgver"] {
        assert!(
            lower.contains(needle),
            "packaging/aur/README.md should document `{needle}` in the publish/update steps"
        );
    }
}

#[test]
fn readme_documents_aur_helper_install() {
    let readme = read("README.md");
    assert!(
        readme.contains("paru -S hyprburst") || readme.contains("yay -S hyprburst"),
        "README should document installing the AUR package with an AUR helper"
    );
}

//! Golden-file tests for Phase 4 of the publish-readiness plan: crates.io
//! publishing preparation.
//!
//! These tests pin the publishing artifacts so they cannot silently regress:
//!   * A `CHANGELOG.md` in Keep a Changelog format must exist, track Semantic
//!     Versioning, and carry an entry for the current crate version.
//!   * The crate is published as `hyprburst` — the bare `burst` name is taken on
//!     crates.io by an unrelated 2017 disassembler, so the project, binary,
//!     window class, and config dirs all align on `hyprburst`.
//!   * `CHANGELOG.md` ships inside the published package.
//!   * `RELEASING.md` documents the real `cargo publish` command plus the
//!     owner-only / burned-version rollback steps.
//!   * `README.md` documents `cargo install hyprburst`.

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

#[test]
fn changelog_follows_keep_a_changelog() {
    let log = read("CHANGELOG.md");
    assert!(
        log.contains("# Changelog"),
        "CHANGELOG.md should have a top-level `# Changelog` heading"
    );
    assert!(
        log.contains("Keep a Changelog"),
        "CHANGELOG.md should follow the Keep a Changelog format and link to it"
    );
    assert!(
        log.contains("Semantic Versioning"),
        "CHANGELOG.md should state it adheres to Semantic Versioning"
    );
}

#[test]
fn changelog_has_entry_for_current_version() {
    let log = read("CHANGELOG.md");
    let version = crate_version();
    assert!(
        log.contains(&format!("[{version}]")) || log.contains(&format!("## {version}")),
        "CHANGELOG.md should carry an entry for the current crate version {version}"
    );
    // At least one of the conventional change groups must be present.
    assert!(
        ["### Added", "### Changed", "### Fixed"]
            .iter()
            .any(|h| log.contains(h)),
        "CHANGELOG.md should group changes under Added/Changed/Fixed headings"
    );
}

#[test]
fn crate_publishes_as_hyprburst() {
    let cargo = read("Cargo.toml");
    assert!(
        cargo.contains("name = \"hyprburst\""),
        "Cargo.toml package name should be the available `hyprburst` (bare `burst` is taken on crates.io)"
    );
    assert!(
        cargo.contains("piny4man/hyprburst"),
        "Cargo.toml repository URL should point at the renamed `hyprburst` repo"
    );
}

#[test]
fn changelog_is_shipped_in_the_package() {
    let cargo = read("Cargo.toml");
    assert!(
        cargo.contains("\"CHANGELOG.md\""),
        "Cargo.toml `include` should ship CHANGELOG.md in the published package"
    );
}

#[test]
fn releasing_doc_covers_publish_command_and_rollback() {
    let doc = read("RELEASING.md");
    let lower = doc.to_lowercase();
    assert!(
        doc.contains("cargo publish"),
        "RELEASING.md should document the exact `cargo publish` command"
    );
    assert!(
        lower.contains("owner") && lower.contains("burned"),
        "RELEASING.md should document the owner-only publish and burned-version rollback steps"
    );
}

#[test]
fn readme_documents_hyprburst_install() {
    let readme = read("README.md");
    assert!(
        readme.contains("cargo install hyprburst"),
        "README.md should document installing the published crate with `cargo install hyprburst`"
    );
}

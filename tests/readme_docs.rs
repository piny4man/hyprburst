//! Golden-file tests for the public-readiness documentation in `README.md`.
//!
//! Phase 2 of the publish-readiness plan requires the README to be sufficient
//! for someone discovering Burst through crates.io or AUR: a product pitch, a
//! demo/screenshots placeholder, install methods (from source, crates.io, AUR),
//! a usage section explaining every command, and a troubleshooting section.
//! These tests fail CI if any of those sections regress out of the README.

use std::path::PathBuf;

fn load_readme() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

#[test]
fn readme_has_demo_placeholder() {
    let readme = load_readme();
    assert!(
        readme.contains("## Demo"),
        "README should have a `## Demo` section (screenshot/demo placeholder) for crates.io/AUR discovery"
    );
}

#[test]
fn readme_has_install_section_with_all_methods() {
    let readme = load_readme();
    assert!(
        readme.contains("## Install"),
        "README should have an `## Install` section"
    );
    assert!(
        readme.contains("cargo install burst"),
        "README should document the `cargo install burst` method"
    );
    assert!(
        readme.contains("cargo install --path ."),
        "README should document installing from source with `cargo install --path .`"
    );
    assert!(
        readme.to_lowercase().contains("aur"),
        "README should document the AUR install method"
    );
}

#[test]
fn readme_explains_every_command() {
    let readme = load_readme();
    assert!(
        readme.contains("## Usage"),
        "README should have a `## Usage` section consolidating the commands"
    );
    assert!(
        readme.contains("burst tui"),
        "README should explain the `burst tui` command"
    );
    assert!(
        readme.contains("--bench-startup"),
        "README should explain the `--bench-startup` flag"
    );
}

#[test]
fn readme_has_troubleshooting_section() {
    let readme = load_readme();
    assert!(
        readme.contains("## Troubleshooting"),
        "README should have a `## Troubleshooting` section for new users"
    );
}

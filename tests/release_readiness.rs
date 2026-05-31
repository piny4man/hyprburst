//! Golden-file tests for Phase 3 of the publish-readiness plan: CI and release
//! workflow readiness.
//!
//! These tests pin the release process so it cannot silently regress:
//!   * CI must run on both pull requests and pushes to `main`, and must run the
//!     same `fmt`/`clippy`/`test` gates a release candidate is held to.
//!   * `Cargo.lock` must stay committed and packaged (binary-crate policy).
//!   * A `RELEASING.md` checklist must cover every release step (version bump,
//!     changelog, tag, crates.io publish, GitHub release, AUR update) and the
//!     `cargo package` / `cargo publish --dry-run` readiness validation.

use std::path::PathBuf;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let path = manifest_dir().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

fn load_ci() -> String {
    read(".github/workflows/ci.yml")
}

#[test]
fn ci_runs_on_pull_requests_and_main_pushes() {
    let ci = load_ci();
    assert!(
        ci.contains("pull_request:"),
        "CI should run on pull requests"
    );
    assert!(ci.contains("push:"), "CI should also run on pushes to main");
    // Both triggers must be scoped to `main`.
    assert!(
        ci.matches("branches: [main]").count() >= 2,
        "both the pull_request and push triggers should target `main`"
    );
}

#[test]
fn ci_runs_fmt_clippy_and_test_gates() {
    let ci = load_ci();
    assert!(
        ci.contains("cargo fmt --check"),
        "CI should check formatting with `cargo fmt --check`"
    );
    assert!(
        ci.contains("cargo clippy --all-targets -- -D warnings"),
        "CI should run clippy with warnings denied"
    );
    assert!(
        ci.contains("cargo test --all-targets"),
        "CI should run the test suite with `cargo test --all-targets`"
    );
}

#[test]
fn cargo_lock_is_committed_and_packaged() {
    let lock = manifest_dir().join("Cargo.lock");
    assert!(
        lock.exists(),
        "Cargo.lock must stay committed for the binary crate (reproducible builds)"
    );
    let cargo_toml = read("Cargo.toml");
    assert!(
        cargo_toml.contains("\"Cargo.lock\""),
        "Cargo.toml `include` should ship Cargo.lock in the published package"
    );
}

#[test]
fn releasing_doc_covers_full_checklist() {
    let doc = read("RELEASING.md");
    let lower = doc.to_lowercase();
    for needle in [
        "version bump",
        "changelog",
        "tag",
        "crates.io",
        "github release",
        "aur",
    ] {
        assert!(
            lower.contains(needle),
            "RELEASING.md should cover `{needle}` in the release checklist"
        );
    }
}

#[test]
fn releasing_doc_documents_package_readiness_validation() {
    let doc = read("RELEASING.md");
    assert!(
        doc.contains("cargo package"),
        "RELEASING.md should document verifying the package with `cargo package`"
    );
    assert!(
        doc.contains("cargo publish --dry-run"),
        "RELEASING.md should document the `cargo publish --dry-run` readiness check"
    );
    assert!(
        doc.contains("cargo publish") && !doc.contains("cargo publish --dry-run\n```\n#"),
        "RELEASING.md should document the real `cargo publish` step too"
    );
}

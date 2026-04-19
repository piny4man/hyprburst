//! Golden-file test for `packaging/hyprland-burst.conf`.
//!
//! Fails CI if the shipped Hyprland config drifts from the documentation —
//! every windowrule promised in the README, the `burst` bind, and the
//! commented `env = TERMINAL,rio` example must all be present.

use std::path::PathBuf;

fn load_conf() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packaging/hyprland-burst.conf");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

#[test]
fn contains_all_documented_windowrules() {
    let conf = load_conf();
    let expected_rules = [
        "windowrule = match:class ^(burst)$, float on",
        "windowrule = match:class ^(burst)$, size 100% 100%",
        "windowrule = match:class ^(burst)$, center on",
        "windowrule = match:class ^(burst)$, opacity 0.9 0.8",
        "windowrule = match:class ^(burst)$, border_size 0",
        "windowrule = match:class ^(burst)$, no_shadow on",
        "windowrule = match:class ^(burst)$, stay_focused on",
        "windowrule = match:class ^(burst)$, dim_around on",
    ];
    for rule in expected_rules {
        assert!(
            conf.contains(rule),
            "hyprland-burst.conf missing windowrule: {rule:?}"
        );
    }
}

#[test]
fn contains_burst_bind() {
    let conf = load_conf();
    assert!(
        conf.lines()
            .any(|l| l.trim() == "bind = SUPER, Space, exec, burst"),
        "hyprland-burst.conf missing `bind = SUPER, Space, exec, burst` — the Hyprland bind"
    );
}

#[test]
fn does_not_reference_removed_burst_launch_wrapper() {
    let conf = load_conf();
    assert!(
        !conf.contains("burst-launch"),
        "hyprland-burst.conf still references the removed `burst-launch` shell wrapper"
    );
}

#[test]
fn does_not_bind_to_removed_launch_subcommand() {
    let conf = load_conf();
    assert!(
        !conf.lines().any(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && t.contains("burst launch")
        }),
        "hyprland-burst.conf still invokes the removed `burst launch` subcommand — use bare `burst`"
    );
}

#[test]
fn contains_commented_terminal_env_example() {
    let conf = load_conf();
    assert!(
        conf.lines()
            .any(|line| line.trim_start().starts_with('#') && line.contains("env = TERMINAL,rio")),
        "hyprland-burst.conf missing commented `env = TERMINAL,rio` example"
    );
}

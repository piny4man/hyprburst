//! Golden-file tests for the shipped Hyprland configs.
//!
//! Fails CI if `packaging/hyprburst.conf` (legacy hyprlang) or
//! `packaging/hyprburst.lua` (Hyprland 0.55+ Lua) drifts from the documentation.
//! Lua is intentionally minimal now: it only binds Super+Space while TOML drives
//! launch-time rules. Legacy hyprlang still ships static fallback rules.

use std::path::PathBuf;

fn load_conf() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packaging/hyprburst.conf");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

fn load_lua() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("packaging/hyprburst.lua");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

fn load_readme() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("README.md");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

#[test]
fn contains_all_documented_windowrules() {
    let conf = load_conf();
    let expected_rules = [
        "windowrule = match:class ^(hyprburst)$, float on",
        "windowrule = match:class ^(hyprburst)$, pin on",
        "windowrule = match:class ^(hyprburst)$, size (monitor_w) (monitor_h)",
        "windowrule = match:class ^(hyprburst)$, move 0 0",
        "windowrule = match:class ^(hyprburst)$, opacity 0.9 0.9",
        "windowrule = match:class ^(hyprburst)$, border_size 0",
        "windowrule = match:class ^(hyprburst)$, no_shadow on",
        "windowrule = match:class ^(hyprburst)$, stay_focused on",
        "windowrule = match:class ^(hyprburst)$, dim_around on",
    ];
    for rule in expected_rules {
        assert!(
            conf.contains(rule),
            "hyprburst.conf missing windowrule: {rule:?}"
        );
    }
}

#[test]
fn does_not_use_percent_size_or_center_for_overlay() {
    let conf = load_conf();
    assert!(
        !conf.contains("size 100% 100%"),
        "hyprburst.conf still uses percentage sizing; use monitor expressions instead"
    );
    assert!(
        !conf.contains("windowrule = match:class ^(hyprburst)$, center on"),
        "hyprburst.conf still centers hyprburst; move it to 0 0 after monitor-sized resize"
    );
}

#[test]
fn readme_documents_full_monitor_overlay_rules() {
    let readme = load_readme();
    assert!(
        readme.contains("placement = \"fullscreen\"") && readme.contains("monitor_w"),
        "README missing TOML-driven fullscreen monitor sizing rule"
    );
    assert!(
        readme.contains("center = true"),
        "README missing centered placement rule"
    );
}

#[test]
fn readme_documents_waybar_layer_workaround() {
    let readme = load_readme();
    assert!(
        readme.contains("\"layer\": \"bottom\""),
        "README should document setting Waybar to the bottom layer when it covers hyprburst"
    );
    assert!(
        readme.contains("Waybar is a layer-shell surface"),
        "README should explain why normal window rules cannot draw above top-layer Waybar"
    );
}

#[test]
fn contains_hyprburst_bind() {
    let conf = load_conf();
    assert!(
        conf.lines()
            .any(|l| l.trim() == "bind = SUPER, Space, exec, hyprburst"),
        "hyprburst.conf missing `bind = SUPER, Space, exec, hyprburst` — the Hyprland bind"
    );
}

#[test]
fn does_not_reference_removed_hyprburst_launch_wrapper() {
    let conf = load_conf();
    assert!(
        !conf.contains("hyprburst-launch"),
        "hyprburst.conf still references the removed `hyprburst-launch` shell wrapper"
    );
}

#[test]
fn does_not_bind_to_removed_launch_subcommand() {
    let conf = load_conf();
    assert!(
        !conf.lines().any(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && t.contains("hyprburst launch")
        }),
        "hyprburst.conf still invokes the removed `hyprburst launch` subcommand — use bare `hyprburst`"
    );
}

#[test]
fn lua_is_minimal_bind_only() {
    let lua = load_lua();
    assert!(
        !lua.contains("hl.window_rule"),
        "hyprburst.lua should not ship static window rules; TOML drives launch-time rules"
    );
    assert!(
        lua.contains("config.toml"),
        "hyprburst.lua should point users at TOML customization"
    );
}

#[test]
fn lua_contains_hyprburst_bind() {
    let lua = load_lua();
    assert!(
        lua.contains("hl.bind(\"SUPER + Space\", hl.dsp.exec_cmd(\"hyprburst\"))"),
        "hyprburst.lua missing the Super+Space bind via hl.dsp.exec_cmd"
    );
}

#[test]
fn readme_documents_both_config_formats() {
    let readme = load_readme();
    assert!(
        readme.contains("hyprburst.lua"),
        "README should document the Lua (hyprburst.lua) config for Hyprland 0.55+"
    );
    assert!(
        readme.contains("hyprburst.conf"),
        "README should document the hyprlang (hyprburst.conf) config"
    );
}

#[test]
fn readme_documents_minimal_lua_and_toml_placement() {
    let readme = load_readme();
    assert!(
        readme.contains("hl.dsp.exec_cmd(\"hyprburst\")"),
        "README should document the minimal Lua bind"
    );
    assert!(
        readme.contains("placement = \"fullscreen\""),
        "README should document TOML placement"
    );
    assert!(
        readme.contains("window.opacity") && readme.contains("override"),
        "README should document opacity as the Hyprland compositor opacity knob"
    );
}

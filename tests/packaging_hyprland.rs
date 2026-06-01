//! Golden-file tests for the shipped Hyprland configs.
//!
//! Fails CI if `packaging/hyprburst.conf` (hyprlang) or `packaging/hyprburst.lua`
//! (Hyprland 0.55+ Lua) drifts from the documentation — every windowrule promised
//! in the README and the `Super+Space` bind must be present in both formats. It
//! also checks that README setup notes document the known Hyprland/Waybar overlay
//! details and both config formats.

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
        "windowrule = match:class ^(hyprburst)$, size (monitor_w) (monitor_h)",
        "windowrule = match:class ^(hyprburst)$, move 0 0",
        "windowrule = match:class ^(hyprburst)$, opacity 0.9 0.8",
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
        readme.contains("size (monitor_w) (monitor_h)"),
        "README missing monitor expression sizing rule"
    );
    assert!(
        readme.contains("move 0 0"),
        "README missing top-left placement rule"
    );
    assert!(
        readme.contains("full-monitor floating overlay"),
        "README should explain that hyprburst is fake-fullscreen, not real fullscreen"
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
fn lua_contains_all_documented_windowrules() {
    let lua = load_lua();
    let expected_effects = [
        "float = true",
        "size = \"monitor_w monitor_h\"",
        "move = \"0 0\"",
        "opacity = \"0.9 0.8\"",
        "border_size = 0",
        "no_shadow = true",
        "stay_focused = true",
        "dim_around = true",
    ];
    for effect in expected_effects {
        assert!(
            lua.contains(effect),
            "hyprburst.lua missing window-rule effect: {effect:?}"
        );
    }
    // Every rule must match the hyprburst app-id.
    assert!(
        lua.contains("match = { class = \"hyprburst\" }"),
        "hyprburst.lua window rules must match the hyprburst app-id"
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

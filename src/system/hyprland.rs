//! Hyprland dispatch compatibility.
//!
//! Hyprland 0.55 deprecated the `hyprlang` config in favor of Lua and, with it,
//! changed `hyprctl dispatch`: the legacy bare-word form (`hyprctl dispatch exec
//! firefox`) is now fed to a Lua evaluator as `hl.dispatch(exec firefox)`, which
//! fails to parse — and, worse, exits `0`, so the launch silently does nothing.
//! The new form routes through the `hl.dsp` namespace
//! (`hyprctl dispatch 'hl.dsp.exec_cmd("firefox")'`).
//!
//! To keep the launcher working across the transition, [`dispatch_syntax`]
//! detects which form the running Hyprland expects (from `hyprctl version`,
//! overridable via `HYPRBURST_DISPATCH`) and the arg builders below produce the
//! matching `hyprctl` invocation. Pre-0.55 stays on the legacy form; 0.55+ uses
//! Lua.

use std::process::Command;
use std::sync::OnceLock;

use crate::domain::config::{Config, WindowPlacement};

/// Which `hyprctl dispatch` form the running Hyprland accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchSyntax {
    /// Pre-0.55 bare-word dispatchers: `dispatch exec -- firefox`.
    Legacy,
    /// 0.55+ Lua dispatchers: `dispatch 'hl.dsp.exec_cmd("firefox")'`.
    Lua,
}

/// Environment override for [`dispatch_syntax`]: set to `lua` or `legacy` to
/// pin the form, bypassing version detection (escape hatch for the config-
/// transition edge where the version alone is ambiguous).
pub const DISPATCH_ENV: &str = "HYPRBURST_DISPATCH";
pub const CHILD_ENV: &str = "HYPRBURST_CHILD";

/// The dispatch form to use, detected once and cached for the process.
pub fn dispatch_syntax() -> DispatchSyntax {
    static CACHE: OnceLock<DispatchSyntax> = OnceLock::new();
    *CACHE.get_or_init(|| {
        if let Some(forced) = std::env::var(DISPATCH_ENV)
            .ok()
            .and_then(|v| parse_forced_syntax(&v))
        {
            return forced;
        }
        hyprctl_version_line()
            .and_then(|line| parse_dispatch_syntax_from_version(&line))
            // No readable version → assume the long-standing legacy form, the
            // behavior every pre-0.55 install relied on.
            .unwrap_or(DispatchSyntax::Legacy)
    })
}

/// Parse a forced [`DispatchSyntax`] from the [`DISPATCH_ENV`] value, if it names
/// one (case-insensitive). Unrecognized values yield `None` (fall back to
/// detection).
fn parse_forced_syntax(value: &str) -> Option<DispatchSyntax> {
    match value.trim().to_ascii_lowercase().as_str() {
        "lua" => Some(DispatchSyntax::Lua),
        "legacy" => Some(DispatchSyntax::Legacy),
        _ => None,
    }
}

/// First line of `hyprctl version`, or `None` if it can't be run.
fn hyprctl_version_line() -> Option<String> {
    let out = Command::new("hyprctl").arg("version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().next().map(str::to_string)
}

/// Decide the dispatch form from a `hyprctl version` first line such as
/// `Hyprland 0.55.2 built from branch …`. Returns `None` when no `MAJOR.MINOR`
/// version can be found, so the caller can fall back. 0.55+ → [`DispatchSyntax::Lua`].
pub fn parse_dispatch_syntax_from_version(line: &str) -> Option<DispatchSyntax> {
    let (major, minor) = parse_major_minor(line)?;
    let lua = (major, minor) >= (0, 55);
    Some(if lua {
        DispatchSyntax::Lua
    } else {
        DispatchSyntax::Legacy
    })
}

/// Find the first `MAJOR.MINOR[.PATCH]` token in `line` (optionally `v`-prefixed)
/// and return `(major, minor)`.
fn parse_major_minor(line: &str) -> Option<(u32, u32)> {
    line.split_whitespace().find_map(|tok| {
        let tok = tok.trim_start_matches('v');
        let mut parts = tok.split('.');
        let major: u32 = parts.next()?.parse().ok()?;
        let minor: u32 = parts.next()?.parse().ok()?;
        Some((major, minor))
    })
}

/// Escape a string for embedding inside a Lua double-quoted literal: backslashes
/// and double quotes only, which covers the command lines and workspace names the
/// launcher dispatches (no embedded newlines).
fn lua_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':'))
    {
        return s.to_string();
    }

    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

fn format_rule_float(value: f32) -> String {
    let mut s = format!("{value:.3}");
    while s.contains('.') && s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.push('0');
    }
    s
}

fn launch_rules_lua(config: &Config) -> String {
    let opacity = format_rule_float(config.window.opacity);
    let common = format!(
        "float = true, pin = true, stay_focused = true, opacity = {}",
        lua_quote(&format!("{opacity} {opacity} override"))
    );

    match config.window.placement {
        WindowPlacement::Fullscreen => {
            // A full-monitor overlay has no visible edges, so drop the border and
            // rounded corners. Centered windows keep the user's Hyprland decoration.
            format!(
                "{{ {common}, size = {{ \"monitor_w\", \"monitor_h\" }}, move = {{ 0, 0 }}, border_size = 0, rounding = 0 }}"
            )
        }
        WindowPlacement::Centered => format!(
            "{{ {common}, size = {{ {}, {} }}, center = true }}",
            config.window.width, config.window.height
        ),
    }
}

fn current_exe_command() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.into_os_string().into_string().ok())
        .unwrap_or_else(|| "hyprburst".to_string())
}

/// Relaunch hyprburst through Hyprland's Lua `exec_cmd(..., rules)` path so the
/// user's TOML placement/opacity choices can be applied without maintaining
/// static window rules in `hyprland.lua`. Returns `true` when the parent process
/// successfully handed off to Hyprland and should exit.
pub fn dispatch_configured_launcher(config: &Config) -> bool {
    if std::env::var_os(CHILD_ENV).is_some() || dispatch_syntax() != DispatchSyntax::Lua {
        return false;
    }

    let command = format!(
        "env {}=1 {}",
        CHILD_ENV,
        shell_quote(&current_exe_command())
    );
    let expression = format!(
        "hl.dsp.exec_cmd({}, {})",
        lua_quote(&command),
        launch_rules_lua(config)
    );

    Command::new("hyprctl")
        .args(["dispatch", &expression])
        .status()
        .is_ok_and(|status| status.success())
}

/// `hyprctl` args to exec `command` under the given dispatch `syntax`.
pub fn exec_dispatch_args(syntax: DispatchSyntax, command: &str) -> Vec<String> {
    match syntax {
        DispatchSyntax::Legacy => vec![
            "dispatch".into(),
            "exec".into(),
            "--".into(),
            command.into(),
        ],
        DispatchSyntax::Lua => vec![
            "dispatch".into(),
            format!("hl.dsp.exec_cmd({})", lua_quote(command)),
        ],
    }
}

/// `hyprctl` args to toggle the special (scratchpad) workspace `name` under the
/// given dispatch `syntax`.
pub fn special_toggle_args(syntax: DispatchSyntax, name: &str) -> Vec<String> {
    match syntax {
        DispatchSyntax::Legacy => vec![
            "dispatch".into(),
            "togglespecialworkspace".into(),
            name.into(),
        ],
        DispatchSyntax::Lua => vec![
            "dispatch".into(),
            format!("hl.dsp.workspace.toggle_special({})", lua_quote(name)),
        ],
    }
}

/// Exec `command` via `hyprctl dispatch`, in whichever form the running Hyprland
/// accepts. Best-effort: a spawn failure (no Hyprland) is ignored, matching the
/// launcher's fire-and-forget launch.
pub fn dispatch_exec(command: &str) {
    let _ = Command::new("hyprctl")
        .args(exec_dispatch_args(dispatch_syntax(), command))
        .spawn();
}

/// Toggle the special workspace `name` via `hyprctl dispatch`, in whichever form
/// the running Hyprland accepts. Best-effort, like [`dispatch_exec`].
pub fn dispatch_toggle_special(name: &str) {
    let _ = Command::new("hyprctl")
        .args(special_toggle_args(dispatch_syntax(), name))
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lua_syntax_for_0_55_and_later() {
        for line in [
            "Hyprland 0.55.2 built from branch v0.55.2 at commit abc clean",
            "Hyprland 0.55.0 built from branch main",
            "Hyprland 0.60.1 built",
            "Hyprland 1.0.0 built",
        ] {
            assert_eq!(
                parse_dispatch_syntax_from_version(line),
                Some(DispatchSyntax::Lua),
                "{line:?} should select the Lua form",
            );
        }
    }

    #[test]
    fn parses_legacy_syntax_before_0_55() {
        for line in [
            "Hyprland 0.54.0 built from branch v0.54.0",
            "Hyprland 0.41.2 built",
            "Hyprland 0.0.9 built",
        ] {
            assert_eq!(
                parse_dispatch_syntax_from_version(line),
                Some(DispatchSyntax::Legacy),
                "{line:?} should select the legacy form",
            );
        }
    }

    #[test]
    fn unparseable_version_returns_none() {
        assert_eq!(parse_dispatch_syntax_from_version("Hyprland built"), None);
        assert_eq!(parse_dispatch_syntax_from_version(""), None);
    }

    #[test]
    fn forced_syntax_overrides_detection() {
        assert_eq!(parse_forced_syntax("lua"), Some(DispatchSyntax::Lua));
        assert_eq!(
            parse_forced_syntax("  LEGACY "),
            Some(DispatchSyntax::Legacy)
        );
        assert_eq!(parse_forced_syntax("nonsense"), None);
    }

    #[test]
    fn legacy_exec_args_match_the_old_form() {
        assert_eq!(
            exec_dispatch_args(DispatchSyntax::Legacy, "firefox"),
            vec!["dispatch", "exec", "--", "firefox"],
        );
    }

    #[test]
    fn lua_exec_args_quote_the_command() {
        assert_eq!(
            exec_dispatch_args(DispatchSyntax::Lua, "firefox --new-window"),
            vec!["dispatch", "hl.dsp.exec_cmd(\"firefox --new-window\")"],
        );
    }

    #[test]
    fn lua_exec_args_escape_quotes_and_backslashes() {
        assert_eq!(
            exec_dispatch_args(DispatchSyntax::Lua, "say \"hi\"\\there"),
            vec!["dispatch", "hl.dsp.exec_cmd(\"say \\\"hi\\\"\\\\there\")"],
        );
    }

    #[test]
    fn special_toggle_args_match_each_form() {
        assert_eq!(
            special_toggle_args(DispatchSyntax::Legacy, "hyprburst"),
            vec!["dispatch", "togglespecialworkspace", "hyprburst"],
        );
        assert_eq!(
            special_toggle_args(DispatchSyntax::Lua, "hyprburst"),
            vec!["dispatch", "hl.dsp.workspace.toggle_special(\"hyprburst\")",],
        );
    }

    #[test]
    fn configured_fullscreen_rules_use_monitor_expressions() {
        let cfg = Config::from_toml_str("[window]\nplacement = \"fullscreen\"\n").unwrap();
        assert_eq!(
            launch_rules_lua(&cfg),
            "{ float = true, pin = true, stay_focused = true, opacity = \"0.85 0.85 override\", size = { \"monitor_w\", \"monitor_h\" }, move = { 0, 0 }, border_size = 0, rounding = 0 }"
        );
    }

    #[test]
    fn default_placement_is_centered() {
        // Default config now centers rather than covering the monitor.
        let cfg = Config::default();
        assert_eq!(
            launch_rules_lua(&cfg),
            "{ float = true, pin = true, stay_focused = true, opacity = \"0.85 0.85 override\", size = { 640, 720 }, center = true }"
        );
    }

    #[test]
    fn configured_centered_rules_use_configured_size() {
        let cfg = Config::from_toml_str(
            "[window]\nplacement = \"centered\"\nwidth = 800\nheight = 600\nopacity = 0.75\n",
        )
        .unwrap();
        assert_eq!(
            launch_rules_lua(&cfg),
            "{ float = true, pin = true, stay_focused = true, opacity = \"0.75 0.75 override\", size = { 800, 600 }, center = true }"
        );
    }

    #[test]
    fn shell_quote_handles_spaces_and_quotes() {
        assert_eq!(shell_quote("/usr/bin/hyprburst"), "/usr/bin/hyprburst");
        assert_eq!(shell_quote("/tmp/my app's/bin"), "'/tmp/my app'\\''s/bin'");
    }
}

use std::fmt;
use std::path::PathBuf;

use crate::config::{Config, TerminalConfig};

/// Argv emitted in place of the `{cmd}` placeholder when resolving a terminal
/// template. `burst tui` — not bare `burst` — because bare `burst` re-execs
/// into a terminal (that's what got us here), which would recurse forever.
const BURST_CMD_ARGV: &[&str] = &["burst", "tui"];
const BUILTIN_CHAIN: &[&str] = &["alacritty", "wezterm", "ghostty", "kitty", "foot", "rio"];

/// Snapshot of terminal-related environment variables.
///
/// Captured up-front (rather than read lazily) so resolution is a pure
/// function of its inputs and trivial to drive from tests.
#[derive(Debug, Default, Clone)]
pub struct Env {
    pub terminal: Option<String>,
    pub term: Option<String>,
    pub term_program: Option<String>,
}

impl Env {
    pub fn from_env() -> Self {
        Self {
            terminal: std::env::var("TERMINAL").ok(),
            term: std::env::var("TERM").ok(),
            term_program: std::env::var("TERM_PROGRAM").ok(),
        }
    }
}

/// Filesystem queries the resolver needs, abstracted so tests don't shell out.
pub trait PathProbe {
    fn is_on_path(&self, bin: &str) -> bool;

    /// Resolve a binary on PATH, following symlinks, and return the final
    /// target's file stem (e.g. `x-terminal-emulator` → `Some("kitty")`).
    /// Returns `None` if the binary is not on PATH.
    fn resolved_name(&self, bin: &str) -> Option<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTerminal {
    pub binary: String,
    pub argv: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError {
    NoTerminalFound,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTerminalFound => write!(
                f,
                "no terminal emulator found on PATH (set $TERMINAL, install one of {:?}, or configure [terminal.preferred])",
                BUILTIN_CHAIN
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Resolve which terminal emulator burst should re-exec into.
///
/// Resolution order (first match wins):
/// 1. `config.terminal.preferred` — explicit user choice
/// 2. `$TERMINAL`
/// 3. `$TERM`, then `$TERM_PROGRAM`
/// 4. `x-terminal-emulator` (symlink resolved to its target)
/// 5. Built-in chain: `alacritty, wezterm, ghostty, kitty, foot, rio`
pub fn resolve(
    config: &Config,
    env: &Env,
    probe: &dyn PathProbe,
) -> Result<ResolvedTerminal, ResolveError> {
    let terminal_cfg = &config.terminal;

    for name in &terminal_cfg.preferred {
        if !name.is_empty() && probe.is_on_path(name) {
            return Ok(make_resolved(name, name, terminal_cfg));
        }
    }

    if let Some(t) = env.terminal.as_deref()
        && !t.is_empty()
        && probe.is_on_path(t)
    {
        return Ok(make_resolved(t, t, terminal_cfg));
    }

    for candidate in [env.term.as_deref(), env.term_program.as_deref()]
        .into_iter()
        .flatten()
    {
        if !candidate.is_empty() && probe.is_on_path(candidate) {
            return Ok(make_resolved(candidate, candidate, terminal_cfg));
        }
    }

    if probe.is_on_path("x-terminal-emulator") {
        let target = probe
            .resolved_name("x-terminal-emulator")
            .unwrap_or_else(|| "x-terminal-emulator".to_string());
        return Ok(make_resolved(&target, &target, terminal_cfg));
    }

    for name in BUILTIN_CHAIN {
        if probe.is_on_path(name) {
            return Ok(make_resolved(name, name, terminal_cfg));
        }
    }

    Err(ResolveError::NoTerminalFound)
}

fn make_resolved(binary: &str, template_key: &str, terminal: &TerminalConfig) -> ResolvedTerminal {
    let template = terminal
        .flags
        .get(template_key)
        .cloned()
        .unwrap_or_else(|| vec!["-e".to_string(), "{cmd}".to_string()]);

    let mut argv = Vec::with_capacity(template.len() + BURST_CMD_ARGV.len());
    for arg in template {
        if arg == "{cmd}" {
            argv.extend(BURST_CMD_ARGV.iter().map(|s| (*s).to_string()));
        } else {
            let substituted = arg.replace("{class}", &terminal.class);
            argv.push(substituted.replace("{cmd}", &BURST_CMD_ARGV.join(" ")));
        }
    }

    ResolvedTerminal {
        binary: binary.to_string(),
        argv,
    }
}

/// Production [`PathProbe`] that scans `$PATH` and canonicalizes symlinks.
pub struct SystemPathProbe;

impl SystemPathProbe {
    fn find(bin: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            let candidate = dir.join(bin);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }
}

impl PathProbe for SystemPathProbe {
    fn is_on_path(&self, bin: &str) -> bool {
        Self::find(bin).is_some()
    }

    fn resolved_name(&self, bin: &str) -> Option<String> {
        let found = Self::find(bin)?;
        let canonical = std::fs::canonicalize(&found).unwrap_or(found);
        canonical
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeProbe {
        // bin name -> resolved file_stem (None = not on PATH)
        entries: HashMap<String, Option<String>>,
    }

    impl FakeProbe {
        fn new() -> Self {
            Self::default()
        }

        fn with(mut self, bin: &str) -> Self {
            self.entries.insert(bin.to_string(), Some(bin.to_string()));
            self
        }

        fn with_symlink(mut self, bin: &str, resolved_to: &str) -> Self {
            self.entries
                .insert(bin.to_string(), Some(resolved_to.to_string()));
            self
        }
    }

    impl PathProbe for FakeProbe {
        fn is_on_path(&self, bin: &str) -> bool {
            self.entries.contains_key(bin)
        }
        fn resolved_name(&self, bin: &str) -> Option<String> {
            self.entries.get(bin).cloned().flatten()
        }
    }

    fn cfg_with_terminal(terminal: TerminalConfig) -> Config {
        Config {
            terminal,
            ..Config::default()
        }
    }

    fn terminal_with(preferred: &[&str]) -> TerminalConfig {
        TerminalConfig {
            preferred: preferred.iter().map(|s| s.to_string()).collect(),
            ..TerminalConfig::default()
        }
    }

    #[test]
    fn config_preferred_wins_over_everything() {
        let cfg = cfg_with_terminal(terminal_with(&["rio", "ghostty"]));
        let env = Env {
            terminal: Some("alacritty".to_string()),
            term: Some("kitty".to_string()),
            term_program: None,
        };
        let probe = FakeProbe::new()
            .with("rio")
            .with("ghostty")
            .with("alacritty")
            .with("kitty");

        let resolved = resolve(&cfg, &env, &probe).unwrap();
        assert_eq!(resolved.binary, "rio");
        assert_eq!(
            resolved.argv,
            vec!["--title=burst", "-e", "burst", "tui"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn config_preferred_falls_through_when_not_installed() {
        let cfg = cfg_with_terminal(terminal_with(&["rio"]));
        let env = Env {
            terminal: Some("alacritty".to_string()),
            ..Env::default()
        };
        let probe = FakeProbe::new().with("alacritty");

        let resolved = resolve(&cfg, &env, &probe).unwrap();
        assert_eq!(resolved.binary, "alacritty");
    }

    #[test]
    fn falls_through_to_terminal_env() {
        let cfg = Config::default();
        let env = Env {
            terminal: Some("kitty".to_string()),
            ..Env::default()
        };
        let probe = FakeProbe::new().with("kitty").with("alacritty");

        let resolved = resolve(&cfg, &env, &probe).unwrap();
        assert_eq!(resolved.binary, "kitty");
        assert_eq!(
            resolved.argv,
            vec![
                "--class=burst".to_string(),
                "burst".to_string(),
                "tui".to_string(),
            ]
        );
    }

    #[test]
    fn empty_terminal_env_is_skipped() {
        let cfg = Config::default();
        let env = Env {
            terminal: Some(String::new()),
            ..Env::default()
        };
        let probe = FakeProbe::new().with("alacritty");

        let resolved = resolve(&cfg, &env, &probe).unwrap();
        assert_eq!(resolved.binary, "alacritty");
    }

    #[test]
    fn falls_through_to_term_env() {
        let cfg = Config::default();
        let env = Env {
            term: Some("ghostty".to_string()),
            ..Env::default()
        };
        let probe = FakeProbe::new().with("ghostty");

        let resolved = resolve(&cfg, &env, &probe).unwrap();
        assert_eq!(resolved.binary, "ghostty");
    }

    #[test]
    fn falls_through_to_term_program_env() {
        let cfg = Config::default();
        let env = Env {
            term: Some("xterm-256color".to_string()),
            term_program: Some("foot".to_string()),
            ..Env::default()
        };
        let probe = FakeProbe::new().with("foot");

        let resolved = resolve(&cfg, &env, &probe).unwrap();
        assert_eq!(resolved.binary, "foot");
    }

    #[test]
    fn falls_through_to_x_terminal_emulator_with_symlink_resolution() {
        let cfg = Config::default();
        let env = Env::default();
        let probe = FakeProbe::new().with_symlink("x-terminal-emulator", "kitty");

        let resolved = resolve(&cfg, &env, &probe).unwrap();
        assert_eq!(resolved.binary, "kitty");
        assert_eq!(
            resolved.argv,
            vec![
                "--class=burst".to_string(),
                "burst".to_string(),
                "tui".to_string(),
            ]
        );
    }

    #[test]
    fn falls_through_to_builtin_chain() {
        let cfg = Config::default();
        let env = Env::default();
        // Only foot is installed.
        let probe = FakeProbe::new().with("foot");

        let resolved = resolve(&cfg, &env, &probe).unwrap();
        assert_eq!(resolved.binary, "foot");
    }

    #[test]
    fn builtin_chain_priority_is_alacritty_first() {
        let cfg = Config::default();
        let env = Env::default();
        // All installed — the chain order should pick alacritty first.
        let probe = FakeProbe::new()
            .with("rio")
            .with("foot")
            .with("kitty")
            .with("ghostty")
            .with("wezterm")
            .with("alacritty");

        let resolved = resolve(&cfg, &env, &probe).unwrap();
        assert_eq!(resolved.binary, "alacritty");
    }

    #[test]
    fn each_builtin_resolves_to_correct_argv() {
        let cases: &[(&str, &[&str])] = &[
            ("alacritty", &["--class=burst", "-e", "burst", "tui"]),
            ("wezterm", &["start", "--class=burst", "--", "burst", "tui"]),
            ("ghostty", &["--class=burst", "-e", "burst", "tui"]),
            ("kitty", &["--class=burst", "burst", "tui"]),
            ("foot", &["--app-id=burst", "burst", "tui"]),
            ("rio", &["--title=burst", "-e", "burst", "tui"]),
        ];

        for (binary, expected) in cases {
            let cfg = cfg_with_terminal(terminal_with(&[binary]));
            let env = Env::default();
            let probe = FakeProbe::new().with(binary);

            let resolved = resolve(&cfg, &env, &probe).unwrap();
            assert_eq!(resolved.binary, *binary);
            let expected_vec: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
            assert_eq!(resolved.argv, expected_vec, "argv mismatch for {}", binary);
        }
    }

    #[test]
    fn custom_flags_override_builtin() {
        let mut terminal = terminal_with(&["alacritty"]);
        terminal.flags.insert(
            "alacritty".to_string(),
            vec![
                "--my-flag".to_string(),
                "--class={class}".to_string(),
                "{cmd}".to_string(),
            ],
        );
        let cfg = cfg_with_terminal(terminal);
        let env = Env::default();
        let probe = FakeProbe::new().with("alacritty");

        let resolved = resolve(&cfg, &env, &probe).unwrap();
        assert_eq!(
            resolved.argv,
            vec![
                "--my-flag".to_string(),
                "--class=burst".to_string(),
                "burst".to_string(),
                "tui".to_string(),
            ]
        );
    }

    #[test]
    fn custom_class_substitutes_into_placeholder() {
        let terminal = TerminalConfig {
            class: "my-launcher".to_string(),
            preferred: vec!["foot".to_string()],
            ..TerminalConfig::default()
        };
        let cfg = cfg_with_terminal(terminal);
        let env = Env::default();
        let probe = FakeProbe::new().with("foot");

        let resolved = resolve(&cfg, &env, &probe).unwrap();
        assert_eq!(
            resolved.argv,
            vec![
                "--app-id=my-launcher".to_string(),
                "burst".to_string(),
                "tui".to_string(),
            ]
        );
    }

    #[test]
    fn user_supplied_terminal_unknown_to_table_uses_minimal_fallback() {
        // A custom terminal not in the built-in flag table and not configured
        // gets the conservative `-e burst` form.
        let cfg = cfg_with_terminal(terminal_with(&["mystery-term"]));
        let env = Env::default();
        let probe = FakeProbe::new().with("mystery-term");

        let resolved = resolve(&cfg, &env, &probe).unwrap();
        assert_eq!(resolved.binary, "mystery-term");
        assert_eq!(
            resolved.argv,
            vec!["-e".to_string(), "burst".to_string(), "tui".to_string()]
        );
    }

    #[test]
    fn cmd_placeholder_expands_to_burst_tui_to_avoid_relaunch_recursion() {
        // Bare `burst` re-execs into a terminal, so the terminal must run
        // `burst tui` (the inline TUI subcommand), not bare `burst` — otherwise
        // the spawned terminal loops back into run_launch().
        for name in ["alacritty", "wezterm", "ghostty", "kitty", "foot", "rio"] {
            let cfg = cfg_with_terminal(terminal_with(&[name]));
            let env = Env::default();
            let probe = FakeProbe::new().with(name);

            let resolved = resolve(&cfg, &env, &probe).unwrap();
            let tail: Vec<&str> = resolved
                .argv
                .iter()
                .rev()
                .take(2)
                .map(String::as_str)
                .collect();
            assert_eq!(
                tail,
                vec!["tui", "burst"],
                "{} template must end with `burst tui`, got {:?}",
                name,
                resolved.argv,
            );
        }
    }

    #[test]
    fn errors_when_no_candidate_on_path() {
        let cfg = Config::default();
        let env = Env::default();
        let probe = FakeProbe::new();

        let err = resolve(&cfg, &env, &probe).unwrap_err();
        assert_eq!(err, ResolveError::NoTerminalFound);
    }
}

use std::io;
use std::process::ExitCode;
use std::time::Instant;

use hyprburst::domain::config::Config;
use hyprburst::gpu::window;
use hyprburst::system::hyprland;
use hyprburst::tui::app::App;
use hyprburst::tui::{input, terminal};

const HELP: &str = "\
hyprburst — a fast application launcher

USAGE:
    hyprburst [COMMAND]

COMMANDS:
    tui       Run the launcher inline in the current terminal (crossterm fallback)
    native    Run the direct in-process GPU frontend (fallback/comparison)
    help      Print this help message

FLAGS:
    -h, --help           Print this help message
    --measure            Open the window, report cold-start + RSS at first frame, then exit
    --bench-startup      Measure config-load + App::new latency and exit

With no command, hyprburst opens its Rio-backed launcher window. Use
`hyprburst native` for the direct in-process frontend or `hyprburst tui` to run
inline in the current terminal.
";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    BenchStartup,
    Rio { measure: bool },
    Native { measure: bool },
    Tui,
    Help,
    Unknown(String),
}

fn parse_command(args: &[String]) -> Command {
    if args.iter().any(|arg| arg == "--bench-startup") {
        return Command::BenchStartup;
    }

    match args.first().map(String::as_str) {
        None => Command::Rio { measure: false },
        Some("--measure") => Command::Rio { measure: true },
        Some("native") => Command::Native {
            measure: args.iter().any(|arg| arg == "--measure"),
        },
        Some("tui") => Command::Tui,
        Some("help" | "--help" | "-h") => Command::Help,
        Some(other) => Command::Unknown(other.to_string()),
    }
}

/// Load config for the launcher, falling back to defaults (with a stderr note)
/// if the file is missing or invalid — so a stale config never blocks the
/// launcher from opening. The note carries the migration hint for removed keys.
fn load_config() -> Config {
    match Config::load() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("hyprburst: {}", err);
            eprintln!("hyprburst: falling back to default configuration");
            Config::default()
        }
    }
}

fn bench_startup() -> io::Result<()> {
    let start = Instant::now();
    let config = load_config();
    let _app = App::new(config);
    let elapsed = start.elapsed();
    println!(
        "hyprburst startup: {:.2}ms",
        elapsed.as_secs_f64() * 1_000.0
    );
    if let Some(kb) = hyprburst::bench::peak_rss_kb() {
        println!("hyprburst peak RSS: {} KB", kb);
    }
    Ok(())
}

/// Run the crossterm TUI inline in the current terminal — the fallback for
/// SSH / no-GPU sessions where the windowed launcher can't open.
fn run_tui() -> io::Result<()> {
    let config = load_config();

    let mut terminal = terminal::init()?;
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        terminal::restore_on_panic();
        original_hook(info);
    }));

    let mut app = App::new(config);
    let mut force_draw = true;
    while app.running {
        // Block up to one frame for the first event, then drain the burst and
        // apply it all before a single redraw.
        let inputs = input::poll_batch()?;
        let got_input = !inputs.is_empty();
        for input in inputs {
            match input {
                input::Input::Key(code) => app.handle_key(code),
                input::Input::Mouse(event) => app.handle_mouse(event),
            }
        }

        if force_draw || got_input || app.effects_running() {
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    frame.render_widget(&mut app, area);
                    app.apply_effects(frame, area);
                })
                .map_err(|err| io::Error::other(format!("terminal draw failed: {err}")))?;
            force_draw = false;
        }
    }

    terminal::restore()
}

/// Open the launcher window. `measure` exits right after the first frame with a
/// cold-start / RSS report.
fn run_window(measure: bool, start: Instant) -> ExitCode {
    let config = load_config();
    if !measure && hyprland::dispatch_configured_launcher_with_args(&config, &["native"]) {
        return ExitCode::SUCCESS;
    }

    match window::run(config, measure, start) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("hyprburst: {}", err);
            ExitCode::FAILURE
        }
    }
}

fn run_rio(measure: bool, start: Instant) -> ExitCode {
    let config = load_config();
    if !measure && hyprland::dispatch_configured_launcher(&config) {
        return ExitCode::SUCCESS;
    }

    match window::run_rio(config, measure, start) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("hyprburst: {err}");
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let start = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();

    match parse_command(&args) {
        Command::BenchStartup => io_to_exit(bench_startup()),
        Command::Rio { measure } => run_rio(measure, start),
        Command::Native { measure } => run_window(measure, start),
        Command::Tui => io_to_exit(run_tui()),
        Command::Help => {
            print!("{}", HELP);
            ExitCode::SUCCESS
        }
        Command::Unknown(other) => {
            eprintln!("hyprburst: unknown command '{}'\n", other);
            eprint!("{}", HELP);
            ExitCode::from(2)
        }
    }
}

/// Map an `io::Result` from a subcommand into a process exit code, printing any
/// error to stderr.
fn io_to_exit(result: io::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("hyprburst: {}", err);
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn bare_command_selects_rio_and_native_is_explicit() {
        assert_eq!(parse_command(&args(&[])), Command::Rio { measure: false });
        assert_eq!(
            parse_command(&args(&["native"])),
            Command::Native { measure: false }
        );
    }

    #[test]
    fn measurement_targets_the_selected_frontend() {
        assert_eq!(
            parse_command(&args(&["--measure"])),
            Command::Rio { measure: true }
        );
        assert_eq!(
            parse_command(&args(&["native", "--measure"])),
            Command::Native { measure: true }
        );
    }
}

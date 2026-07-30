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
    rio       Prototype the launcher through a rio-vt managed PTY
    help      Print this help message

FLAGS:
    -h, --help           Print this help message
    --measure            Open the window, report cold-start + RSS at first frame, then exit
    --bench-startup      Measure config-load + App::new latency and exit

With no command, hyprburst opens its launcher window (GPU-rendered, owns its own
Wayland surface). Use `hyprburst tui` to run inline in the current terminal or
`hyprburst rio` to exercise the experimental rio-vt PTY frontend.
";

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

    while app.running {
        terminal
            .draw(|frame| {
                let area = frame.area();
                frame.render_widget(&mut app, area);
                app.apply_effects(frame, area);
            })
            .expect("terminal draw failed");
        if let Some(code) = input::poll_key()? {
            app.handle_key(code);
        }
    }

    terminal::restore()
}

/// Open the launcher window. `measure` exits right after the first frame with a
/// cold-start / RSS report.
fn run_window(measure: bool, start: Instant) -> ExitCode {
    let config = load_config();
    if !measure && hyprland::dispatch_configured_launcher(&config) {
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
    if !measure && hyprland::dispatch_configured_launcher_with_args(&config, &["rio"]) {
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

    if args.iter().any(|a| a == "--bench-startup") {
        return io_to_exit(bench_startup());
    }

    if args.first().is_some_and(|arg| arg == "rio") {
        return run_rio(args.iter().any(|arg| arg == "--measure"), start);
    }

    if args.iter().any(|a| a == "--measure") {
        return run_window(true, start);
    }

    match args.first().map(String::as_str) {
        None => run_window(false, start),
        Some("tui") => io_to_exit(run_tui()),
        Some("help" | "--help" | "-h") => {
            print!("{}", HELP);
            ExitCode::SUCCESS
        }
        Some(other) => {
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

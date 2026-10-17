use std::io;
#[cfg(any(test, feature = "hotpath"))]
use std::path::{Path, PathBuf};
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
    Tui { gpu_host: bool },
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
        Some("tui") => Command::Tui {
            gpu_host: args.iter().any(|arg| arg == "--gpu-host"),
        },
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

/// Adapt only the base palette for Rio's internal child; external terminals
/// receive the resolved config unchanged.
fn tui_config(mut config: Config, gpu_host: bool) -> Config {
    if gpu_host {
        // Internal Rio child mode: ANSI default colors are resolved by the
        // parent, preserving its base panel opacity. Keep selection/accent
        // colors explicit, just as in the direct native renderer.
        config.colors.foreground = ratatui::style::Color::Reset;
        config.colors.background = ratatui::style::Color::Reset;
    }
    config
}

/// Where the Rio child (`tui --gpu-host`) writes its hotpath report: next to
/// the parent's `HOTPATH_OUTPUT_PATH` with a `-tui-<pid>` suffix, or a temp
/// file. The child's stdout is the parent's PTY, so hotpath's default stdout
/// report would be painted into the launcher and lost.
#[cfg(any(test, feature = "hotpath"))]
fn child_report_path(parent: Option<&Path>, pid: u32) -> PathBuf {
    let Some(parent) = parent else {
        return std::env::temp_dir().join(format!("hyprburst-hotpath-tui-{pid}.txt"));
    };
    let stem = parent.file_stem().unwrap_or_default().to_string_lossy();
    let name = match parent.extension() {
        Some(ext) => format!("{stem}-tui-{pid}.{}", ext.to_string_lossy()),
        None => format!("{stem}-tui-{pid}"),
    };
    parent.with_file_name(name)
}

/// Point the Rio child's hotpath report at its own file and keep its metrics
/// server (and the "port busy" notice it prints) off the parent's PTY.
#[cfg(feature = "hotpath")]
fn redirect_child_report() {
    let parent = std::env::var_os("HOTPATH_OUTPUT_PATH").map(PathBuf::from);
    let path = child_report_path(parent.as_deref(), std::process::id());
    // SAFETY: called first thing in `main`, before any thread is spawned, so
    // nothing can read the environment concurrently.
    unsafe {
        std::env::set_var("HOTPATH_OUTPUT_PATH", path);
        std::env::set_var("HOTPATH_METRICS_SERVER_OFF", "1");
    }
}

/// Run the crossterm TUI inline in the current terminal — the fallback for
/// SSH / no-GPU sessions where the windowed launcher can't open.
///
/// Built with `--features hotpath` this is the profiling scope for the TUI and
/// the Rio child; see docs/hotpath-profiling.md.
fn run_tui(gpu_host: bool) -> io::Result<()> {
    let _profile = profile_scope("hyprburst::tui");
    let config = tui_config(load_config(), gpu_host);
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

    match open_native(config, measure, start) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("hyprburst: {}", err);
            ExitCode::FAILURE
        }
    }
}

/// The native window's profiling scope. Kept after the Hyprland hand-off so a
/// dispatching parent never writes a report of its own.
fn open_native(
    config: Config,
    measure: bool,
    start: Instant,
) -> Result<(), Box<dyn std::error::Error>> {
    let _profile = profile_scope("hyprburst::native");
    window::run(config, measure, start)
}

/// The Rio window's profiling scope (the parent half; the child profiles
/// itself in [`run_tui`]).
fn open_rio(
    config: Config,
    measure: bool,
    start: Instant,
) -> Result<(), Box<dyn std::error::Error>> {
    let _profile = profile_scope("hyprburst::rio");
    window::run_rio(config, measure, start)
}

// `#[hotpath::main]` would install this per annotated function; the binary
// has three profiling scopes, so it is declared once here instead.
#[cfg(feature = "hotpath-alloc")]
#[global_allocator]
static ALLOCATOR: hotpath::CountingAllocator = hotpath::CountingAllocator::new();

/// Start a hotpath profiling scope: the report is written when the returned
/// guard drops (or after `HOTPATH_SHUTDOWN_MS`). A no-op without the
/// `hotpath` feature.
fn profile_scope(caller: &'static str) -> hotpath::HotpathGuard {
    hotpath::HotpathGuardBuilder::new(caller)
        .percentiles(&[50.0, 95.0, 99.0])
        .build()
}

fn run_rio(measure: bool, start: Instant) -> ExitCode {
    let config = load_config();
    if !measure && hyprland::dispatch_configured_launcher(&config) {
        return ExitCode::SUCCESS;
    }

    match open_rio(config, measure, start) {
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
        Command::Tui { gpu_host } => {
            #[cfg(feature = "hotpath")]
            if gpu_host {
                redirect_child_report();
            }
            io_to_exit(run_tui(gpu_host))
        }
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
    fn gpu_host_tui_is_distinct_from_external_terminal() {
        assert_eq!(
            parse_command(&args(&["tui", "--gpu-host"])),
            Command::Tui { gpu_host: true }
        );
        assert_eq!(
            parse_command(&args(&["tui"])),
            Command::Tui { gpu_host: false }
        );
    }

    #[test]
    fn only_gpu_host_defers_base_colors_and_keeps_selection_explicit() {
        let config = Config::from_toml_str(
            "[colors]\nforeground = \"#123456\"\nbackground = \"#654321\"\nselected = \"red\"\nselected_bg = \"blue\"",
        ).unwrap();
        assert_eq!(tui_config(config.clone(), false), config);
        let mut expected = config.clone();
        expected.colors.foreground = ratatui::style::Color::Reset;
        expected.colors.background = ratatui::style::Color::Reset;
        assert_eq!(tui_config(config, true), expected);
    }

    #[test]
    fn rio_child_report_sits_next_to_the_parent_report() {
        assert_eq!(
            child_report_path(Some(Path::new("/tmp/hb.json")), 42),
            PathBuf::from("/tmp/hb-tui-42.json")
        );
        assert_eq!(
            child_report_path(Some(Path::new("/tmp/hb")), 42),
            PathBuf::from("/tmp/hb-tui-42")
        );
        assert_eq!(
            child_report_path(None, 42),
            std::env::temp_dir().join("hyprburst-hotpath-tui-42.txt")
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

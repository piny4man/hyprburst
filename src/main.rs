use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::Instant;

use hyprburst::app::App;
use hyprburst::config::Config;
use hyprburst::terminal_resolver::{self, Env, ResolveError, SystemPathProbe};
use hyprburst::{input, terminal};

const HELP: &str = "\
hyprburst — a fast application launcher

USAGE:
    hyprburst [COMMAND]

COMMANDS:
    tui       Run the TUI inline in the current terminal (no re-exec)
    help      Print this help message

FLAGS:
    -h, --help           Print this help message
    --bench-startup      Measure config-load + App::new latency and exit
    --bench              Run the benchmark harness and print the comparison table

With no command, hyprburst re-execs into the user's preferred terminal emulator.
";

fn load_config_for_tui() -> Config {
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
    let config = load_config_for_tui();
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

fn run_tui() -> io::Result<()> {
    let config = load_config_for_tui();

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

/// Re-exec hyprburst inside the user's preferred terminal emulator.
///
/// Loads config, snapshots the environment, resolves the terminal, then
/// `execvp`s into `<binary> <argv...>`. Only returns on error — on success
/// the process image is replaced.
fn run_launch() -> ! {
    let config = match Config::load() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("hyprburst: config parse failed: {}", err);
            std::process::exit(1);
        }
    };

    let env = Env::from_env();
    let resolved = match terminal_resolver::resolve(&config, &env, &SystemPathProbe) {
        Ok(r) => r,
        Err(err @ ResolveError::NoTerminalFound) => {
            eprintln!("hyprburst: {}", err);
            std::process::exit(1);
        }
    };

    let err = Command::new(&resolved.binary).args(&resolved.argv).exec();
    eprintln!(
        "hyprburst: failed to exec {} {}: {}",
        resolved.binary,
        resolved.argv.join(" "),
        err
    );
    std::process::exit(1);
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--bench-startup") {
        return bench_startup();
    }

    if args.iter().any(|a| a == "--bench") {
        print!("{}", hyprburst::bench::run_baseline_report());
        return Ok(());
    }

    match args.first().map(String::as_str) {
        None => run_launch(),
        Some("tui") => run_tui(),
        Some("help" | "--help" | "-h") => {
            print!("{}", HELP);
            Ok(())
        }
        Some(other) => {
            eprintln!("hyprburst: unknown command '{}'\n", other);
            eprint!("{}", HELP);
            std::process::exit(2);
        }
    }
}

use std::io;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};

use burst::app::App;
use burst::config::Config;
use burst::effects::{EffectPrototype, PrototypeEffect, all_prototypes};
use burst::terminal_resolver::{self, Env, ResolveError, SystemPathProbe};
use burst::{input, terminal};
use ratatui::crossterm::event::KeyCode;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};

const HELP: &str = "\
burst — a fast application launcher

USAGE:
    burst [COMMAND]

COMMANDS:
    tui       Run the TUI inline in the current terminal (no re-exec)
    effect-demo <name>
              Preview a tachyonfx prototype inline
    help      Print this help message

FLAGS:
    -h, --help           Print this help message
    --bench-startup      Measure config-load + App::new latency and exit

With no command, burst re-execs into the user's preferred terminal emulator.
";

fn load_config_for_tui() -> Config {
    match Config::load() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("burst: {}", err);
            eprintln!("burst: falling back to default configuration");
            Config::default()
        }
    }
}

fn bench_startup() -> io::Result<()> {
    let start = Instant::now();
    let config = load_config_for_tui();
    let _app = App::new(config);
    let elapsed = start.elapsed();
    println!("burst startup: {:.2}ms", elapsed.as_secs_f64() * 1_000.0);
    if let Some(kb) = read_self_rss_kb() {
        println!("burst peak RSS: {} KB", kb);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn read_self_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            return rest.split_whitespace().next().and_then(|n| n.parse().ok());
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn read_self_rss_kb() -> Option<u64> {
    None
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

fn run_effect_demo(name: &str) -> io::Result<()> {
    let Some(prototype) = EffectPrototype::from_name(name) else {
        eprintln!("burst: unknown effect prototype '{}'", name);
        eprintln!(
            "burst: available prototypes: {}",
            prototype_names().join(", ")
        );
        std::process::exit(2);
    };

    let mut terminal = terminal::init()?;
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        terminal::restore_on_panic();
        original_hook(info);
    }));

    let mut effect = PrototypeEffect::new(prototype);
    loop {
        terminal
            .draw(|frame| {
                let area = frame.area();
                render_effect_demo_frame(frame, area, prototype);
                effect.apply(frame, area);
            })
            .expect("terminal draw failed");

        if effect.is_done() {
            std::thread::sleep(Duration::from_millis(250));
            break;
        }

        if matches!(input::poll_key()?, Some(KeyCode::Esc | KeyCode::Char('q'))) {
            break;
        }
    }

    terminal::restore()
}

fn prototype_names() -> Vec<&'static str> {
    all_prototypes()
        .into_iter()
        .map(EffectPrototype::name)
        .collect()
}

fn render_effect_demo_frame(frame: &mut Frame<'_>, area: Rect, prototype: EffectPrototype) {
    let block = Block::new()
        .title(format!(" tachyonfx prototype: {} ", prototype.name()))
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Cyan));

    let text = vec![
        Line::from(Span::styled(
            "Burst effect prototype demo",
            Style::new()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("Prompt: > fire"),
        Line::from(""),
        Line::from("Results:"),
        Line::from("  Firefox"),
        Line::from("  Files"),
        Line::from("  Firejail"),
        Line::from(""),
        Line::from("Press q or Esc to exit."),
    ];

    frame.render_widget(Paragraph::new(text).block(block), area);
}

/// Re-exec burst inside the user's preferred terminal emulator.
///
/// Loads config, snapshots the environment, resolves the terminal, then
/// `execvp`s into `<binary> <argv...>`. Only returns on error — on success
/// the process image is replaced.
fn run_launch() -> ! {
    let config = match Config::load() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("burst: config parse failed: {}", err);
            std::process::exit(1);
        }
    };

    let env = Env::from_env();
    let resolved = match terminal_resolver::resolve(&config, &env, &SystemPathProbe) {
        Ok(r) => r,
        Err(err @ ResolveError::NoTerminalFound) => {
            eprintln!("burst: {}", err);
            std::process::exit(1);
        }
    };

    let err = Command::new(&resolved.binary).args(&resolved.argv).exec();
    eprintln!(
        "burst: failed to exec {} {}: {}",
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

    match args.first().map(String::as_str) {
        None => run_launch(),
        Some("tui") => run_tui(),
        Some("effect-demo") => match args.get(1) {
            Some(name) => run_effect_demo(name),
            None => {
                eprintln!("burst: effect-demo requires a prototype name");
                eprintln!(
                    "burst: available prototypes: {}",
                    prototype_names().join(", ")
                );
                std::process::exit(2);
            }
        },
        Some("help" | "--help" | "-h") => {
            print!("{}", HELP);
            Ok(())
        }
        Some(other) => {
            eprintln!("burst: unknown command '{}'\n", other);
            eprint!("{}", HELP);
            std::process::exit(2);
        }
    }
}

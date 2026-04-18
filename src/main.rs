use std::io;
use std::time::Instant;

use burst::app::App;
use burst::config::Config;

fn load_config() -> Config {
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
    let config = load_config();
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

#[cfg(feature = "terminal")]
fn run() -> io::Result<()> {
    use burst::{input, terminal};

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
        if let Some(event) = input::poll_event()? {
            app.handle_event(&event);
        }
    }

    terminal::restore()
}

#[cfg(all(feature = "window", not(feature = "terminal")))]
fn run() -> io::Result<()> {
    Err(io::Error::other(
        "the `window` backend is not yet implemented; build with `--features terminal` for now",
    ))
}

#[cfg(not(any(feature = "terminal", feature = "window")))]
compile_error!("burst requires at least one of the `terminal` or `window` features");

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--bench-startup") {
        return bench_startup();
    }
    run()
}

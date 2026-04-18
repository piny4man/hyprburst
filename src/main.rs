mod app;
mod config;
mod desktop;
mod history;
mod icon;
mod input;
mod launcher;
mod search;
mod terminal;

use std::io;

use app::App;
use config::Config;

fn run() -> io::Result<()> {
    let config = match Config::load() {
        Ok(cfg) => cfg,
        Err(err) => {
            eprintln!("burst: {}", err);
            eprintln!("burst: falling back to default configuration");
            Config::default()
        }
    };

    let mut terminal = terminal::init()?;
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        terminal::restore_on_panic();
        original_hook(info);
    }));

    let mut app = App::new(config);

    while app.running {
        terminal
            .draw(|frame| frame.render_widget(&mut app, frame.area()))
            .expect("terminal draw failed");
        if let Some(event) = input::poll_event()? {
            app.handle_event(&event);
        }
    }

    terminal::restore()
}

fn main() -> io::Result<()> {
    run()
}

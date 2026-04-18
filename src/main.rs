mod app;
mod desktop;
mod history;
mod icon;
mod input;
mod launcher;
mod search;
mod terminal;

use std::io;

use app::App;

fn run() -> io::Result<()> {
    let mut terminal = terminal::init()?;
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        terminal::restore_on_panic();
        original_hook(info);
    }));

    let mut app = App::new();

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

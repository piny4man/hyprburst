use std::io::{self, stdout};

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

pub fn init() -> io::Result<Terminal<impl Backend>> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore() -> io::Result<()> {
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

pub fn restore_on_panic() {
    let _ = restore();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;

    #[test]
    fn restore_does_not_panic() {
        let result = panic::catch_unwind(restore);
        assert!(result.is_ok(), "restore() should not panic");
    }

    #[test]
    fn restore_on_panic_does_not_panic() {
        let result = panic::catch_unwind(restore_on_panic);
        assert!(result.is_ok(), "restore_on_panic() should not panic");
    }

    #[test]
    fn init_restore_lifecycle() {
        let terminal = init();
        if terminal.is_err() {
            return;
        }

        let result = restore();
        assert!(result.is_ok(), "restore() should succeed after init()");
    }

    #[test]
    fn restore_without_init_does_not_panic() {
        let result = panic::catch_unwind(|| {
            let _ = restore();
        });
        assert!(result.is_ok(), "restore() without init should not panic");
    }
}

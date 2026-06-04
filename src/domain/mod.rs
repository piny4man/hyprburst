//! Frontend-agnostic launcher core: the state machine and the data it drives on.
//!
//! Nothing here renders. [`launcher_core`] owns the state machine and abstract
//! [`LauncherAction`](launcher_core::LauncherAction) vocabulary; the rest are the
//! pure utilities it composes — [`config`] (settings schema), [`desktop`] (`.desktop`
//! discovery), [`search`] (ranking), [`history`] (SQLite persistence), and [`icon`]
//! (glyph/theme resolution).

pub mod config;
pub mod desktop;
pub mod history;
pub mod icon;
pub mod launcher_core;
pub mod search;

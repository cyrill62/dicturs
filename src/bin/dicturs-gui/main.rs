//! `dicturs-gui`: graphical UI (egui).

mod gui;
mod theme;

use anyhow::Result;
use dicturs::config::Config;

fn main() -> Result<()> {
    let fresh = Config::is_fresh()?;
    gui::run(Config::load()?, fresh)
}

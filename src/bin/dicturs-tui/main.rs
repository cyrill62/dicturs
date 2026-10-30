//! `dicturs-tui`: terminal UI.

mod tui;

use anyhow::Result;
use dicturs::config::Config;

fn main() -> Result<()> {
    let fresh = Config::is_fresh()?;
    tui::run(Config::load()?, fresh)
}

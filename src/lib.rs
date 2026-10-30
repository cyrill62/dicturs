//! Dicturs core, shared by the `dicturs` (CLI), `dicturs-tui` and `dicturs-gui` binaries.

/// The release version (Cargo.toml, kept in sync with VERSION by scripts/bump-version.sh).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod audio;
pub mod config;
pub mod executor;
pub mod internal;
pub mod launcher;
pub mod listen;
pub mod matcher;
pub mod models;
pub mod stt;

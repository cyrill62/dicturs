//! System dark/light mode, followed live.
//!
//! winit does not report the color scheme on Linux, so it is read here: from the XDG desktop
//! portal when it answers (GNOME, KDE…), otherwise from GNOME's gsettings, which compositors
//! without a settings portal (Hyprland, Sway…) and tools like nwg-look keep up to date.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};

pub use dark_light::Mode;

/// The system mode and where its changes come from. Dropping it stops watching.
pub struct SystemTheme {
    pub initial: Mode,
    source: Source,
}

enum Source {
    Portal(dark_light::Watcher),
    /// `gsettings monitor` child and the modes parsed from its output.
    Gsettings(Child, Receiver<Mode>),
    None,
}

impl SystemTheme {
    pub fn watch() -> Self {
        let portal = dark_light::detect().unwrap_or(Mode::Unspecified);
        if portal != Mode::Unspecified || !cfg!(target_os = "linux") {
            let source = dark_light::subscribe().map_or(Source::None, Source::Portal);
            return Self {
                initial: portal,
                source,
            };
        }
        // No portal answer: gsettings (Hyprland, Sway…).
        let initial = gsettings_mode().unwrap_or(Mode::Unspecified);
        Self {
            initial,
            source: monitor_gsettings().unwrap_or(Source::None),
        }
    }

    /// The latest change since the previous call, if any.
    pub fn try_recv(&self) -> Option<Mode> {
        match &self.source {
            Source::Portal(w) => w.try_iter().last(),
            Source::Gsettings(_, rx) => rx.try_iter().last(),
            Source::None => None,
        }
    }
}

impl Drop for SystemTheme {
    fn drop(&mut self) {
        if let Source::Gsettings(child, _) = &mut self.source {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn monitor_gsettings() -> Option<Source> {
    let mut child = Command::new("gsettings")
        .args(["monitor", "org.gnome.desktop.interface", "color-scheme"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // Lines look like: color-scheme: 'prefer-dark'
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let value = line.rsplit(':').next().unwrap_or_default();
            if tx
                .send(color_scheme_mode(value).unwrap_or_else(gtk_theme_mode))
                .is_err()
            {
                break;
            }
        }
    });
    Some(Source::Gsettings(child, rx))
}

fn gsettings(key: &str) -> Option<String> {
    let out = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", key])
        .output()
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .trim_matches('\'')
            .to_string()
    })
}

fn gsettings_mode() -> Option<Mode> {
    let scheme = gsettings("color-scheme")?;
    Some(color_scheme_mode(&scheme).unwrap_or_else(gtk_theme_mode))
}

/// `prefer-dark` / `prefer-light`; `None` for `default` (no explicit preference).
fn color_scheme_mode(value: &str) -> Option<Mode> {
    match value.trim().trim_matches('\'') {
        "prefer-dark" => Some(Mode::Dark),
        "prefer-light" => Some(Mode::Light),
        _ => None,
    }
}

/// Without an explicit preference, a theme such as `Adwaita-dark` still tells.
fn gtk_theme_mode() -> Mode {
    match gsettings("gtk-theme") {
        Some(theme) if theme.to_lowercase().contains("dark") => Mode::Dark,
        _ => Mode::Light,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_color_scheme() {
        assert_eq!(color_scheme_mode("'prefer-dark'"), Some(Mode::Dark));
        assert_eq!(color_scheme_mode(" 'prefer-light'"), Some(Mode::Light));
        assert_eq!(color_scheme_mode("'default'"), None);
    }
}


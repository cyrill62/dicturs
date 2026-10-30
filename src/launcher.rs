//! dmenu-style launchers (wofi, rofi, fuzzel, ...) and their installation.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};

use crate::config::{Config, VoiceCommand};
use crate::executor::which;

pub struct Launcher {
    pub name: &'static str,
    pub binary: &'static str,
    /// Arguments putting the launcher in dmenu mode; `{prompt}` is replaced.
    pub args: &'static [&'static str],
    pub wayland: bool,
    pub x11: bool,
}

pub const LAUNCHERS: &[Launcher] = &[
    Launcher {
        name: "wofi",
        binary: "wofi",
        args: &["--dmenu", "-i", "-p", "{prompt}"],
        wayland: true,
        x11: false,
    },
    Launcher {
        name: "rofi",
        binary: "rofi",
        args: &["-dmenu", "-i", "-p", "{prompt}"],
        wayland: true,
        x11: true,
    },
    Launcher {
        name: "fuzzel",
        binary: "fuzzel",
        args: &["--dmenu", "-p", "{prompt} "],
        wayland: true,
        x11: false,
    },
    Launcher {
        name: "tofi",
        binary: "tofi",
        args: &["--prompt-text", "{prompt} "],
        wayland: true,
        x11: false,
    },
    Launcher {
        name: "bemenu",
        binary: "bemenu",
        args: &["-i", "-p", "{prompt}"],
        wayland: true,
        x11: true,
    },
    Launcher {
        name: "walker",
        binary: "walker",
        args: &["--dmenu", "-p", "{prompt}"],
        wayland: true,
        x11: false,
    },
    Launcher {
        name: "dmenu",
        binary: "dmenu",
        args: &["-i", "-p", "{prompt}"],
        wayland: false,
        x11: true,
    },
    Launcher {
        name: "fzf",
        binary: "fzf",
        args: &["--prompt", "{prompt} > "],
        wayland: true,
        x11: true,
    },
];

pub fn find(name: &str) -> Option<&'static Launcher> {
    LAUNCHERS.iter().find(|l| l.name == name)
}

pub fn is_installed(l: &Launcher) -> bool {
    which(l.binary).is_some()
}

/// Resolves the `launcher` setting: a known name or a custom dmenu-compatible command line.
fn launcher_argv(setting: &str, prompt: &str) -> Result<Vec<String>> {
    if let Some(l) = find(setting.trim()) {
        if !is_installed(l) {
            bail!(
                "{} is not installed (try `dicturs launcher install {}`)",
                l.name,
                l.name
            );
        }
        let mut v = vec![l.binary.to_string()];
        v.extend(l.args.iter().map(|a| a.replace("{prompt}", prompt)));
        return Ok(v);
    }
    let v = shlex::split(&setting.replace("{prompt}", prompt))
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("invalid launcher command '{setting}'"))?;
    Ok(v)
}

/// Shows `entries` in the launcher, returns the selected index.
pub fn pick(setting: &str, prompt: &str, entries: &[String]) -> Result<Option<usize>> {
    let argv = launcher_argv(setting, prompt)?;
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting {}", argv[0]))?;
    {
        let mut stdin = child.stdin.take().expect("piped stdin");
        for e in entries {
            writeln!(stdin, "{e}")?;
        }
    }
    let out = child.wait_with_output()?;
    let choice = String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string();
    if choice.is_empty() {
        return Ok(None);
    }
    Ok(entries.iter().position(|e| *e == choice))
}

pub fn menu_entry(cfg: &Config, c: &VoiceCommand, show_state: bool) -> String {
    let state = if show_state {
        if c.enabled && cfg.category_active(&c.category) {
            "[on]  "
        } else {
            "[off] "
        }
    } else {
        ""
    };
    let cat = if c.category.is_empty() {
        String::new()
    } else {
        format!("{} › ", c.category)
    };
    format!("{state}{cat}{}  ⇒  {}  #{}", c.keyword, c.command, c.id)
}

// ---------- installation ----------

pub struct PackageManager {
    pub name: &'static str,
    pub binary: &'static str,
    pub install: &'static [&'static str],
}

pub const PACKAGE_MANAGERS: &[PackageManager] = &[
    PackageManager {
        name: "emerge",
        binary: "emerge",
        install: &["emerge", "--noreplace", "--verbose"],
    },
    PackageManager {
        name: "pacman",
        binary: "pacman",
        install: &["pacman", "-S", "--needed", "--noconfirm"],
    },
    PackageManager {
        name: "apt",
        binary: "apt-get",
        install: &["apt-get", "install", "-y"],
    },
    PackageManager {
        name: "dnf",
        binary: "dnf",
        install: &["dnf", "install", "-y"],
    },
    PackageManager {
        name: "zypper",
        binary: "zypper",
        install: &["zypper", "--non-interactive", "install"],
    },
    PackageManager {
        name: "xbps",
        binary: "xbps-install",
        install: &["xbps-install", "-y"],
    },
    PackageManager {
        name: "apk",
        binary: "apk",
        install: &["apk", "add"],
    },
];

pub fn detect_package_manager() -> Option<&'static PackageManager> {
    PACKAGE_MANAGERS.iter().find(|p| which(p.binary).is_some())
}

/// Package name of a launcher for a package manager (`None` = not packaged).
pub fn package_name(launcher: &str, pm: &str) -> Option<&'static str> {
    Some(match (launcher, pm) {
        ("wofi", "emerge") => "gui-apps/wofi",
        ("rofi", "emerge") => "x11-misc/rofi",
        ("fuzzel", "emerge") => "gui-apps/fuzzel",
        ("tofi", "emerge") => "gui-apps/tofi",
        ("bemenu", "emerge") => "dev-libs/bemenu",
        ("dmenu", "emerge") => "x11-misc/dmenu",
        ("fzf", "emerge") => "app-shells/fzf",
        ("walker", "emerge") => return None,
        ("dmenu", "apt") => "suckless-tools",
        ("rofi", "dnf") => "rofi-wayland",
        ("tofi" | "walker", "pacman" | "apt" | "dnf" | "zypper" | "apk") => return None,
        (name, _) => find(name)?.name,
    })
}

/// Full install command line (with sudo/doas when not root).
pub fn install_command(launcher: &str) -> Result<Vec<String>> {
    let pm =
        detect_package_manager().ok_or_else(|| anyhow!("no supported package manager found"))?;
    let pkg = package_name(launcher, pm.name).ok_or_else(|| {
        anyhow!(
            "{launcher} is not packaged for {}; install it manually",
            pm.name
        )
    })?;
    let mut argv = Vec::new();
    #[cfg(unix)]
    let is_root = {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
    };
    #[cfg(not(unix))]
    let is_root = true;
    if !is_root {
        let elevate = ["sudo", "doas"]
            .into_iter()
            .find(|b| which(b).is_some())
            .ok_or_else(|| anyhow!("sudo or doas is required to install packages"))?;
        argv.push(elevate.to_string());
    }
    argv.extend(pm.install.iter().map(|s| s.to_string()));
    argv.push(pkg.to_string());
    Ok(argv)
}

/// Runs the installer attached to the terminal (sudo may prompt for a password).
pub fn install(launcher: &str) -> Result<()> {
    let argv = install_command(launcher)?;
    println!("$ {}", argv.join(" "));
    let status = Command::new(&argv[0]).args(&argv[1..]).status()?;
    if !status.success() {
        bail!("installation failed ({status})");
    }
    Ok(())
}

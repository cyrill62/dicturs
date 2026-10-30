//! Runs commands through the user's shell so aliases and functions are available.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Shell families with known alias-loading invocations.
pub const KNOWN_SHELLS: &[&str] = &[
    "auto",
    "sh",
    "bash",
    "zsh",
    "fish",
    "nu",
    "powershell",
    "cmd",
];

/// Resolves the `shell` setting into a program + arguments (the command is appended last).
pub fn shell_argv(setting: &str) -> Vec<String> {
    let setting = setting.trim();
    let name = if setting.is_empty() || setting == "auto" {
        std::env::var("SHELL")
            .ok()
            .and_then(|s| s.rsplit('/').next().map(str::to_string))
            .unwrap_or_else(|| if cfg!(windows) { "powershell" } else { "sh" }.into())
    } else {
        setting.to_string()
    };
    let owned = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
    match name.as_str() {
        // Interactive mode reads ~/.bashrc / ~/.zshrc, where aliases usually live.
        "bash" => owned(&["bash", "-i", "-c"]),
        "zsh" => owned(&["zsh", "-i", "-c"]),
        // fish always loads config.fish.
        "fish" => owned(&["fish", "-c"]),
        // `nu -c` skips the user config: pass it explicitly so aliases/defs exist
        // (autoload files are sourced in `wrap_command`).
        "nu" | "nushell" => {
            let mut argv = vec!["nu".to_string()];
            let dir = nushell_config_dir();
            for (flag, file) in [("--env-config", "env.nu"), ("--config", "config.nu")] {
                let path = dir.join(file);
                if path.exists() {
                    argv.push(flag.into());
                    argv.push(path.to_string_lossy().into_owned());
                }
            }
            argv.push("-c".into());
            argv
        }
        "sh" | "dash" | "ksh" => vec![name, "-c".into()],
        // Windows: PowerShell loads the user profile, where aliases live.
        "powershell" | "pwsh" => vec![name, "-Command".into()],
        "cmd" => owned(&["cmd", "/C"]),
        // Custom prefix, e.g. `nu --login -c`.
        custom => match shlex::split(custom) {
            Some(v) if !v.is_empty() => v,
            _ => owned(&["sh", "-c"]),
        },
    }
}

fn nushell_config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join("nushell");
    }
    directories::BaseDirs::new()
        .map(|d| d.config_dir().join("nushell"))
        .unwrap_or_else(|| PathBuf::from("~/.config/nushell"))
}

/// `nu -c` does not read `autoload/*.nu` either, where aliases often live:
/// source them in front of the command.
fn wrap_command(argv: &[String], command: &str) -> String {
    let is_nu = argv[0] == "nu" || argv[0].ends_with("/nu");
    if !is_nu || argv.iter().any(|a| a == "-n" || a == "--no-config-file") {
        return command.to_string();
    }
    let Ok(entries) = std::fs::read_dir(nushell_config_dir().join("autoload")) else {
        return command.to_string();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "nu"))
        .collect();
    files.sort();
    let mut out = String::new();
    for f in files {
        out.push_str(&format!("source '{}'; ", f.display()));
    }
    out.push_str(command);
    out
}

fn build(shell: &str, command: &str) -> Result<Command> {
    if command.trim().is_empty() {
        bail!("empty command");
    }
    let argv = shell_argv(shell);
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).arg(wrap_command(&argv, command));
    Ok(cmd)
}

/// Starts `command` detached (own process group, no stdio), returns its pid.
pub fn spawn_detached(shell: &str, command: &str) -> Result<u32> {
    let mut cmd = build(shell, command)?;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let child = cmd
        .spawn()
        .with_context(|| format!("spawning `{command}`"))?;
    Ok(child.id())
}

/// Runs `command` in the foreground with the terminal attached.
pub fn run_foreground(shell: &str, command: &str) -> Result<std::process::ExitStatus> {
    build(shell, command)?
        .status()
        .with_context(|| format!("running `{command}`"))
}

pub fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let file = format!("{bin}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&path)
        .map(|dir| dir.join(&file))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shells() {
        assert_eq!(shell_argv("zsh"), ["zsh", "-i", "-c"]);
        assert_eq!(shell_argv("nu").first().unwrap(), "nu");
        assert_eq!(shell_argv("nu").last().unwrap(), "-c");
        assert_eq!(shell_argv("nu --login -c"), ["nu", "--login", "-c"]);
    }
}

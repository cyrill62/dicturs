//! whisper.cpp ggml model catalog and downloader.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

pub struct Model {
    pub name: &'static str,
    pub size_mb: u32,
    pub note: &'static str,
}

pub const MODELS: &[Model] = &[
    Model {
        name: "tiny",
        size_mb: 75,
        note: "fastest, lowest accuracy",
    },
    Model {
        name: "tiny.en",
        size_mb: 75,
        note: "English only",
    },
    Model {
        name: "base",
        size_mb: 142,
        note: "good default for commands",
    },
    Model {
        name: "base.en",
        size_mb: 142,
        note: "English only",
    },
    Model {
        name: "small",
        size_mb: 466,
        note: "better accuracy, still CPU friendly",
    },
    Model {
        name: "small.en",
        size_mb: 466,
        note: "English only",
    },
    Model {
        name: "medium",
        size_mb: 1530,
        note: "slow on CPU",
    },
    Model {
        name: "large-v3-turbo-q5_0",
        size_mb: 547,
        note: "near large quality, quantized",
    },
    Model {
        name: "large-v3-turbo",
        size_mb: 1620,
        note: "best speed/quality with a GPU",
    },
    Model {
        name: "large-v3",
        size_mb: 3100,
        note: "most accurate, heavy",
    },
];

pub fn find(name: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.name == name)
}

pub fn models_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("DICTURS_MODELS_DIR") {
        return Ok(PathBuf::from(dir));
    }
    Ok(crate::config::project_dirs()?.data_dir().join("models"))
}

pub fn model_path(name: &str) -> Result<PathBuf> {
    Ok(models_dir()?.join(format!("ggml-{name}.bin")))
}

pub fn is_installed(name: &str) -> bool {
    model_path(name).is_ok_and(|p| p.is_file())
}

pub fn url(name: &str) -> String {
    format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{name}.bin")
}

/// Partial download file, used to show progress.
pub fn partial_path(name: &str) -> Result<PathBuf> {
    Ok(models_dir()?.join(format!("ggml-{name}.bin.part")))
}

/// Downloads a model with curl (or wget). `quiet` hides the progress bar (TUI).
pub fn download(name: &str, quiet: bool) -> Result<PathBuf> {
    if find(name).is_none() {
        bail!("unknown model '{name}'");
    }
    let dest = model_path(name)?;
    let part = partial_path(name)?;
    std::fs::create_dir_all(dest.parent().expect("models dir"))?;
    let url = url(name);
    let mut cmd = if crate::executor::which("curl").is_some() {
        let mut c = Command::new("curl");
        // `url_effective` on stdout tells which host failed after redirects.
        c.args(["-L", "--fail", "-C", "-", "-w", "%{url_effective}", "-o"])
            .arg(&part)
            .arg(&url);
        c.arg(if quiet { "-sS" } else { "-#" });
        c
    } else if crate::executor::which("wget").is_some() {
        let mut c = Command::new("wget");
        c.args(["-c", "-O"]).arg(&part).arg(&url);
        if quiet {
            c.arg("-q");
        }
        c
    } else {
        bail!("curl or wget is required to download models");
    };
    // Progress bar goes straight to the terminal; the TUI keeps errors for its status line.
    cmd.stdout(Stdio::piped()).stderr(if quiet {
        Stdio::piped()
    } else {
        Stdio::inherit()
    });
    let out = cmd
        .spawn()
        .context("starting downloader")?
        .wait_with_output()?;
    if !out.status.success() {
        let last_url = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let mut msg = String::from("download failed");
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if !err.is_empty() {
            msg = format!("{msg}: {err}");
        }
        for u in [last_url.as_str(), url.as_str()] {
            if let Some(hint) = blocked_host_hint(u) {
                msg = format!("{msg}\n{hint}");
                break;
            }
        }
        bail!(msg);
    }
    std::fs::rename(&part, &dest)?;
    Ok(dest)
}

/// Detects hosts that resolve to 0.0.0.0 / loopback, the signature of a DNS blocklist
/// (AdGuard Home, Pi-hole, hosts file...), and explains it.
fn blocked_host_hint(url: &str) -> Option<String> {
    use std::net::ToSocketAddrs;
    let host = url.split("://").nth(1)?.split(['/', '?', ':']).next()?;
    let addrs: Vec<_> = (host, 443).to_socket_addrs().ok()?.collect();
    let blocked = !addrs.is_empty()
        && addrs
            .iter()
            .all(|a| a.ip().is_unspecified() || a.ip().is_loopback());
    blocked.then(|| {
        format!(
            "hint: {host} resolves to {} — it is blocked by your DNS resolver (blocklist). \
             Allow-list it, then retry; the download resumes where it stopped.",
            addrs[0].ip()
        )
    })
}

pub fn remove(name: &str) -> Result<()> {
    let p = model_path(name)?;
    std::fs::remove_file(&p).with_context(|| format!("removing {}", p.display()))
}

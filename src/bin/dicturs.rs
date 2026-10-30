//! `dicturs`: command-line interface. `dicturs` alone opens the terminal UI (`dicturs-tui`).

use std::sync::mpsc;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use dicturs::config::{self, Config};
use dicturs::{audio, executor, internal, launcher, listen, matcher, models, stt};

#[derive(Parser)]
#[command(
    name = "dicturs",
    version,
    about = "Dicturs: run commands with your voice"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the terminal UI (default; runs `dicturs-tui`)
    Tui,
    /// Open the graphical UI (runs `dicturs-gui`)
    Gui,
    /// Listen to the microphone and run matching commands
    Listen {
        /// Only print matches, do not execute
        #[arg(long)]
        dry_run: bool,
        /// Stop after the first recognized command
        #[arg(long)]
        once: bool,
    },
    /// Simulate a transcription: match a phrase and run the command
    Say {
        phrase: Vec<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Transcribe an audio file with the configured engine (needs ffmpeg), then match it
    Transcribe {
        file: std::path::PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
    /// Execute a command by id (ignores its enabled state)
    Exec { id: u32 },
    /// List voice commands
    #[command(alias = "ls")]
    List {
        /// Only this category (and its subcategories)
        #[arg(short, long)]
        category: Option<String>,
        /// Only commands that can fire (enabled, in active categories)
        #[arg(long)]
        active: bool,
        #[arg(long)]
        json: bool,
    },
    /// Add a voice command: dicturs add "open browser" "firefox" -c Apps/Web
    Add {
        keyword: String,
        command: String,
        #[arg(short, long, default_value = "")]
        category: String,
        /// Create it disabled
        #[arg(long)]
        disabled: bool,
    },
    /// Edit a voice command
    Edit {
        id: u32,
        #[arg(short, long)]
        keyword: Option<String>,
        #[arg(short = 'x', long)]
        command: Option<String>,
        #[arg(short, long)]
        category: Option<String>,
    },
    /// Toggle commands on/off
    Toggle { ids: Vec<u32> },
    /// Enable commands
    Enable { ids: Vec<u32> },
    /// Disable commands
    Disable { ids: Vec<u32> },
    /// Delete commands
    #[command(alias = "rm")]
    Remove { ids: Vec<u32> },
    /// Manage categories (paths like Apps/Web)
    #[command(alias = "cat")]
    Category {
        #[command(subcommand)]
        cmd: CategoryCmd,
    },
    /// Pick a command with a launcher (wofi, rofi, ...) and run it
    Menu {
        /// Launcher name or custom dmenu-like command (default: settings)
        #[arg(short, long)]
        launcher: Option<String>,
        /// Toggle the picked command instead of running it
        #[arg(long)]
        toggle: bool,
        /// Print the picked command instead of running it
        #[arg(long)]
        print: bool,
    },
    /// Manage launchers (wofi, rofi, fuzzel, ...)
    Launcher {
        #[command(subcommand)]
        cmd: LauncherCmd,
    },
    /// Manage whisper models
    Model {
        #[command(subcommand)]
        cmd: ModelCmd,
    },
    /// Show or change settings
    #[command(alias = "set")]
    Settings {
        #[command(subcommand)]
        cmd: Option<SettingsCmd>,
    },
    /// List audio input devices
    Devices,
    /// Add the built-in commands (enabled) and samples (disabled) you don't have yet
    #[command(alias = "samples")]
    Seed {
        /// Only show the samples
        #[arg(long)]
        list: bool,
        /// Keyword language: en, fr or system (from your locale)
        #[arg(long, default_value = "en")]
        lang: String,
    },
    /// First launch: create the config with sample commands in a language
    Init {
        /// Keyword language of the samples: en, fr or system (from your locale)
        #[arg(long, default_value = "en")]
        lang: String,
        /// Start with no commands at all
        #[arg(long)]
        no_samples: bool,
    },
}

#[derive(Subcommand)]
enum CategoryCmd {
    #[command(alias = "ls")]
    List,
    Add {
        path: String,
    },
    #[command(alias = "rm")]
    Remove {
        path: String,
    },
    Rename {
        from: String,
        to: String,
    },
    Toggle {
        path: String,
    },
    Enable {
        path: String,
    },
    Disable {
        path: String,
    },
}

#[derive(Subcommand)]
enum LauncherCmd {
    #[command(alias = "ls")]
    List,
    /// Install a launcher with the system package manager
    Install { name: String },
    /// Use this launcher for `dicturs menu`
    Use { name: String },
}

#[derive(Subcommand)]
enum ModelCmd {
    #[command(alias = "ls")]
    List,
    Download {
        name: String,
    },
    /// Select the model (downloads it if needed)
    Use {
        name: String,
    },
    #[command(alias = "rm")]
    Remove {
        name: String,
    },
}

#[derive(Subcommand)]
enum SettingsCmd {
    Show,
    Get {
        key: String,
    },
    /// Set a value, e.g. `dicturs settings set language system`
    Set {
        key: String,
        value: String,
    },
    /// Print the config file path
    Path,
}

/// Replaces this process with another dicturs binary, preferably the one next to `dicturs`.
fn exec_sibling(name: &str) -> Result<()> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let path = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(&file)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| file.into());
    let mut cmd = std::process::Command::new(&path);
    #[cfg(unix)]
    let err = std::os::unix::process::CommandExt::exec(&mut cmd);
    #[cfg(not(unix))]
    let err = match cmd.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(e) => e,
    };
    bail!(
        "cannot run {name}: {err} (build it with the matching feature, e.g. `cargo install --path . --features {}`)",
        name.trim_start_matches("dicturs-")
    )
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let fresh = Config::is_fresh()?;
    let mut cfg = Config::load()?;
    match cli.cmd.unwrap_or(Cmd::Tui) {
        Cmd::Tui => exec_sibling("dicturs-tui")?,
        Cmd::Gui => exec_sibling("dicturs-gui")?,
        Cmd::Init { lang, no_samples } => {
            if !fresh {
                bail!(
                    "{} already exists; use `dicturs seed --lang {lang}` to add the samples",
                    Config::path()?.display()
                );
            }
            let cfg = Config::seeded((!no_samples).then_some(lang.as_str()));
            cfg.save()?;
            println!(
                "created {} with {} command(s): built-ins on, samples off",
                Config::path()?.display(),
                cfg.commands.len()
            );
        }
        Cmd::Listen { dry_run, once } => listen_cli(&cfg, dry_run, once)?,
        Cmd::Say { phrase, dry_run } => {
            if dispatch(&mut cfg, &phrase.join(" "), dry_run)?.is_none() {
                std::process::exit(1);
            }
        }
        Cmd::Transcribe { file, dry_run } => {
            let samples = audio::decode_file(&file)?;
            let hints = cfg.recognition_hints();
            let start = std::time::Instant::now();
            let text = stt::build(&cfg.settings, &hints)?.transcribe(&samples)?;
            eprintln!("heard: \"{text}\" ({} ms)", start.elapsed().as_millis());
            dispatch(&mut cfg, &text, dry_run)?;
        }
        Cmd::Exec { id } => {
            let c = cfg
                .commands
                .iter()
                .find(|c| c.id == id)
                .ok_or_else(|| anyhow::anyhow!("unknown id {id}"))?;
            let command = matcher::expand(&c.command, "");
            if internal::is_internal(&command) {
                run_command(&mut cfg, &command)?;
                return Ok(());
            }
            let status = executor::run_foreground(&cfg.settings.shell, &command)?;
            std::process::exit(status.code().unwrap_or(1));
        }
        Cmd::List {
            category,
            active,
            json,
        } => {
            let cat = category
                .map(|c| config::normalize_category(&c))
                .transpose()?
                .unwrap_or_default();
            let items: Vec<_> = cfg
                .commands_in(&cat, true)
                .filter(|c| !active || (c.enabled && cfg.category_active(&c.category)))
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else {
                for c in items {
                    let on = c.enabled && cfg.category_active(&c.category);
                    let mark = if on {
                        "✔"
                    } else if c.enabled {
                        "◌"
                    } else {
                        "✘"
                    };
                    println!(
                        "{mark} {:>3}  {:<22} {:<28} {}",
                        c.id, c.category, c.keyword, c.command
                    );
                }
            }
        }
        Cmd::Add {
            keyword,
            command,
            category,
            disabled,
        } => {
            let id = cfg.add_command(&keyword, &command, &category, !disabled)?;
            cfg.save()?;
            println!("added #{id}");
        }
        Cmd::Edit {
            id,
            keyword,
            command,
            category,
        } => {
            cfg.update_command(
                id,
                keyword.as_deref(),
                command.as_deref(),
                category.as_deref(),
            )?;
            cfg.save()?;
        }
        Cmd::Toggle { ids } => each(&mut cfg, ids, |cfg, id| {
            let on = cfg.toggle_command(id)?;
            println!("#{id} {}", if on { "enabled" } else { "disabled" });
            Ok(())
        })?,
        Cmd::Enable { ids } => each(&mut cfg, ids, |cfg, id| {
            let _: () = cfg.command_mut(id)?.enabled = true;
            Ok(())
        })?,
        Cmd::Disable { ids } => each(&mut cfg, ids, |cfg, id| {
            let _: () = cfg.command_mut(id)?.enabled = false;
            Ok(())
        })?,
        Cmd::Remove { ids } => each(&mut cfg, ids, |cfg, id| {
            let c = cfg.remove_command(id)?;
            println!("removed #{id} ({})", c.keyword);
            Ok(())
        })?,
        Cmd::Category { cmd } => category_cli(&mut cfg, cmd)?,
        Cmd::Menu {
            launcher,
            toggle,
            print,
        } => menu_cli(&mut cfg, launcher, toggle, print)?,
        Cmd::Launcher { cmd } => match cmd {
            LauncherCmd::List => {
                let pm = launcher::detect_package_manager();
                for l in launcher::LAUNCHERS {
                    let current = if cfg.settings.launcher == l.name {
                        "*"
                    } else {
                        " "
                    };
                    let state = if launcher::is_installed(l) {
                        "installed"
                    } else {
                        "-"
                    };
                    let pkg = pm
                        .and_then(|pm| launcher::package_name(l.name, pm.name))
                        .unwrap_or("(not packaged)");
                    let session = match (l.wayland, l.x11) {
                        (true, true) => "wayland+x11",
                        (true, false) => "wayland",
                        _ => "x11",
                    };
                    println!(
                        "{current} {:<8} {:<10} {:<12} {pkg}",
                        l.name, state, session
                    );
                }
                if let Some(pm) = pm {
                    println!("\npackage manager: {}", pm.name);
                }
            }
            LauncherCmd::Install { name } => launcher::install(&name)?,
            LauncherCmd::Use { name } => {
                cfg.set_setting("launcher", &name)?;
                cfg.save()?;
            }
        },
        Cmd::Model { cmd } => match cmd {
            ModelCmd::List => {
                for m in models::MODELS {
                    let current = if cfg.settings.whisper_model == m.name {
                        "*"
                    } else {
                        " "
                    };
                    let state = if models::is_installed(m.name) {
                        "downloaded"
                    } else {
                        "-"
                    };
                    println!(
                        "{current} {:<22} {:>6} MB  {:<11} {}",
                        m.name, m.size_mb, state, m.note
                    );
                }
                println!("\nmodels dir: {}", models::models_dir()?.display());
            }
            ModelCmd::Download { name } => {
                let p = models::download(&name, false)?;
                println!("saved to {}", p.display());
            }
            ModelCmd::Use { name } => {
                cfg.set_setting("whisper_model", &name)?;
                if !models::is_installed(&name) {
                    models::download(&name, false)?;
                }
                cfg.save()?;
            }
            ModelCmd::Remove { name } => models::remove(&name)?,
        },
        Cmd::Settings { cmd } => match cmd.unwrap_or(SettingsCmd::Show) {
            SettingsCmd::Show => {
                for k in Config::SETTING_KEYS {
                    let v = cfg.get_setting(k)?;
                    let extra = match *k {
                        "language" if v == "system" => {
                            format!("  (→ {})", cfg.settings.effective_language())
                        }
                        "shell" => format!("  (→ {})", executor::shell_argv(&v).join(" ")),
                        _ => String::new(),
                    };
                    println!("{k:<18} = {v}{extra}");
                }
            }
            SettingsCmd::Get { key } => println!("{}", cfg.get_setting(&key)?),
            SettingsCmd::Set { key, value } => {
                cfg.set_setting(&key, &value)?;
                cfg.save()?;
            }
            SettingsCmd::Path => println!("{}", Config::path()?.display()),
        },
        Cmd::Seed { list, lang } => {
            let lang = config::sample_language(&lang);
            if list {
                for (cat, en, fr, cmd) in config::SAMPLES {
                    let k = if lang == "fr" { fr } else { en };
                    println!("{cat:<22} {k:<26} {cmd}");
                }
            } else {
                let b = cfg.add_builtins(&lang);
                let n = cfg.add_samples(&lang);
                cfg.save()?;
                println!(
                    "seeded {b} built-in command(s) (on) and {n} sample(s) (off — enable them with `dicturs enable <id>` or in the TUI)"
                );
            }
        }
        Cmd::Devices => {
            for d in audio::list_input_devices()? {
                let mark = if d.is_default { "*" } else { " " };
                println!("{mark} {:<40} {}", d.id, d.description);
            }
        }
    }
    Ok(())
}

fn each(
    cfg: &mut Config,
    ids: Vec<u32>,
    mut f: impl FnMut(&mut Config, u32) -> Result<()>,
) -> Result<()> {
    if ids.is_empty() {
        bail!("no id given");
    }
    for id in ids {
        f(cfg, id)?;
    }
    cfg.save()
}

fn category_cli(cfg: &mut Config, cmd: CategoryCmd) -> Result<()> {
    match cmd {
        CategoryCmd::List => {
            for c in &cfg.categories {
                let depth = c.path.matches(config::CATEGORY_SEP).count();
                let name = c
                    .path
                    .rsplit(config::CATEGORY_SEP)
                    .next()
                    .unwrap_or(&c.path);
                let n = cfg.commands_in(&c.path, true).count();
                let mark = if cfg.category_active(&c.path) {
                    "✔"
                } else if c.enabled {
                    "◌"
                } else {
                    "✘"
                };
                println!("{mark} {}{name}  ({n})", "  ".repeat(depth));
            }
            return Ok(());
        }
        CategoryCmd::Add { path } => {
            cfg.ensure_category(&path)?;
        }
        CategoryCmd::Remove { path } => cfg.remove_category(&path)?,
        CategoryCmd::Rename { from, to } => cfg.rename_category(&from, &to)?,
        CategoryCmd::Toggle { path } => {
            let on = cfg.toggle_category(&path)?;
            println!("{path} {}", if on { "enabled" } else { "disabled" });
        }
        CategoryCmd::Enable { path } => cfg.set_category_enabled(&path, true)?,
        CategoryCmd::Disable { path } => cfg.set_category_enabled(&path, false)?,
    }
    cfg.save()
}

fn menu_cli(
    cfg: &mut Config,
    launcher_name: Option<String>,
    toggle: bool,
    print: bool,
) -> Result<()> {
    let setting = launcher_name.unwrap_or_else(|| cfg.settings.launcher.clone());
    let list: Vec<_> = if toggle {
        cfg.commands.iter().collect()
    } else {
        cfg.active_commands().collect()
    };
    let entries: Vec<String> = list
        .iter()
        .map(|c| launcher::menu_entry(cfg, c, toggle))
        .collect();
    let ids: Vec<u32> = list.iter().map(|c| c.id).collect();
    let prompt = if toggle { "toggle" } else { "voice cmd" };
    let Some(i) = launcher::pick(&setting, prompt, &entries)? else {
        return Ok(());
    };
    let id = ids[i];
    if toggle {
        let on = cfg.toggle_command(id)?;
        cfg.save()?;
        println!("#{id} {}", if on { "enabled" } else { "disabled" });
    } else {
        let c = cfg.commands.iter().find(|c| c.id == id).expect("listed id");
        let command = matcher::expand(&c.command, "");
        if print {
            println!("{command}");
        } else {
            run_command(cfg, &command)?;
        }
    }
    Ok(())
}

/// Runs an (expanded) command: built-ins are handled here, others go to the shell.
fn run_command(cfg: &mut Config, command: &str) -> Result<internal::Effect> {
    if internal::is_internal(command) {
        let (effect, msg) = internal::run(cfg, command)?;
        eprintln!("[{msg}]");
        return Ok(effect);
    }
    executor::spawn_detached(&cfg.settings.shell, command)?;
    Ok(internal::Effect::None)
}

/// Matches a transcription and runs the command. `None` when nothing matched.
/// Built-ins run even in dry-run mode (so "dry run" can turn it off).
fn dispatch(cfg: &mut Config, text: &str, dry_run: bool) -> Result<Option<internal::Effect>> {
    let command = match matcher::evaluate(cfg, text) {
        matcher::Outcome::Matched(m) => {
            let command = matcher::expand(&m.command.command, &m.args);
            println!(
                "→ \"{}\" ({:.0}%) ⇒ {command}",
                m.command.keyword,
                m.score * 100.0
            );
            command
        }
        other => {
            eprintln!("{}", other.reason(cfg));
            return Ok(None);
        }
    };
    if dry_run && !internal::is_internal(&command) {
        return Ok(Some(internal::Effect::None));
    }
    match run_command(cfg, &command) {
        Ok(effect) => Ok(Some(effect)),
        Err(e) => {
            eprintln!("error: {e:#}");
            Ok(Some(internal::Effect::None))
        }
    }
}

fn listen_cli(cfg: &Config, mut dry_run: bool, once: bool) -> Result<()> {
    let mut cfg = cfg.clone();
    // Restarted when a built-in changes the model or the language.
    loop {
        let (tx, rx) = mpsc::channel();
        let _listener = listen::Listener::start(&cfg, tx)?;
        let mut restart = false;
        for ev in rx.iter() {
            match ev {
                listen::ListenEvent::Status(s) => eprintln!("[{s}]"),
                listen::ListenEvent::Error(e) => eprintln!("error: {e}"),
                listen::ListenEvent::Transcribed { text, took } => {
                    if text.is_empty() {
                        continue;
                    }
                    // Reload the config on each utterance so CLI/TUI edits apply live.
                    if let Ok(fresh) = Config::load() {
                        cfg = fresh;
                    }
                    eprintln!("heard: \"{text}\" ({} ms)", took.as_millis());
                    let Some(effect) = dispatch(&mut cfg, &text, dry_run)? else {
                        continue;
                    };
                    match effect {
                        internal::Effect::StopListening => return Ok(()),
                        internal::Effect::ToggleDryRun => {
                            dry_run = !dry_run;
                            eprintln!("[dry-run {}]", if dry_run { "on" } else { "off" });
                        }
                        internal::Effect::RestartListening => restart = true,
                        internal::Effect::None => {}
                    }
                    if once || restart {
                        break;
                    }
                }
                _ => {}
            }
        }
        if !restart {
            return Ok(());
        }
    }
}

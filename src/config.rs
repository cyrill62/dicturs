use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

/// Separator used in category paths, e.g. `Apps/Browsers`.
pub const CATEGORY_SEP: char = '/';

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub settings: Settings,
    pub categories: Vec<Category>,
    pub commands: Vec<VoiceCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `whisper` (built-in whisper.cpp) or `external` (any STT program).
    pub engine: EngineKind,
    /// Whisper model name, see `dicturs model list`.
    pub whisper_model: String,
    /// Language code for recognition: `system` (from the locale), `auto`
    /// (whisper detection) or a code such as `fr`, `en`.
    pub language: String,
    /// Number of CPU threads used by whisper.
    pub threads: u32,
    /// External STT command; `{wav}` is replaced by a 16 kHz mono WAV path.
    /// The program must print the transcription on stdout.
    pub external_command: String,
    /// Input device name (as shown by `dicturs devices`); empty = system default.
    pub input_device: String,
    /// RMS level (0..1) above which audio is considered speech.
    pub vad_threshold: f32,
    /// Silence duration (ms) that closes an utterance.
    pub silence_ms: u32,
    /// Utterances longer than this are cut (ms).
    pub max_utterance_ms: u32,
    /// Minimal fuzzy score (0..1) to accept a keyword match.
    pub match_threshold: f64,
    /// Optional wake word that must prefix every voice command.
    pub wake_word: String,
    /// Launcher used by `dicturs menu` (wofi, rofi, fuzzel, ...).
    pub launcher: String,
    /// Shell running the commands: `auto` ($SHELL), `sh`, `bash`, `zsh`,
    /// `fish`, `nu` or a custom prefix such as `nu --login -c`.
    pub shell: String,
    /// Start listening as soon as the TUI opens.
    pub listen_on_start: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    Whisper,
    External,
}

impl std::fmt::Display for EngineKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EngineKind::Whisper => "whisper",
            EngineKind::External => "external",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Category {
    /// Full path, e.g. `Apps/Browsers`.
    pub path: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceCommand {
    pub id: u32,
    pub keyword: String,
    pub command: String,
    #[serde(default)]
    pub category: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            engine: EngineKind::Whisper,
            whisper_model: "base".into(),
            language: "system".into(),
            threads: std::thread::available_parallelism()
                .map(|n| n.get().min(8) as u32)
                .unwrap_or(4),
            external_command:
                "whisper-cli -m ~/.local/share/whisper/ggml-base.bin -nt -np -f {wav}".into(),
            input_device: String::new(),
            vad_threshold: 0.02,
            silence_ms: 700,
            max_utterance_ms: 8000,
            match_threshold: 0.8,
            wake_word: String::new(),
            launcher: "wofi".into(),
            shell: "auto".into(),
            listen_on_start: false,
        }
    }
}

/// Sample commands: (category, English keyword, French keyword, command).
/// Plain commands only (no `$VAR`, `&&`, `$(…)`) so they work in sh, zsh, fish and nu.
pub const SAMPLES: &[(&str, &str, &str, &str)] = &[
    (
        "Apps",
        "open terminal",
        "ouvre le terminal",
        "xdg-terminal-exec",
    ),
    (
        "Apps",
        "open file manager",
        "ouvre les fichiers",
        "xdg-open ~",
    ),
    (
        "Apps/Web",
        "open browser",
        "ouvre le navigateur",
        "xdg-open https://duckduckgo.com",
    ),
    (
        "Apps/Web",
        "open youtube",
        "ouvre youtube",
        "xdg-open https://www.youtube.com",
    ),
    (
        "Apps/Web",
        "open mail",
        "ouvre les mails",
        "xdg-open mailto:",
    ),
    (
        "Apps/Web",
        "search for",
        "recherche",
        "xdg-open \"https://duckduckgo.com/?q={args_url}\"",
    ),
    ("Desktop", "type", "écris", "wtype \"{args}\""),
    (
        "System/Audio",
        "volume up",
        "monte le son",
        "wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+",
    ),
    (
        "System/Audio",
        "volume down",
        "baisse le son",
        "wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-",
    ),
    (
        "System/Audio",
        "mute",
        "coupe le son",
        "wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle",
    ),
    (
        "System/Audio",
        "mute microphone",
        "coupe le micro",
        "wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle",
    ),
    (
        "Media",
        "play pause",
        "lecture pause",
        "playerctl play-pause",
    ),
    ("Media", "next track", "morceau suivant", "playerctl next"),
    (
        "Media",
        "previous track",
        "morceau précédent",
        "playerctl previous",
    ),
    ("Media", "stop music", "arrête la musique", "playerctl stop"),
    (
        "System/Display",
        "brightness up",
        "plus de lumière",
        "brightnessctl set 10%+",
    ),
    (
        "System/Display",
        "brightness down",
        "moins de lumière",
        "brightnessctl set 10%-",
    ),
    (
        "System/Session",
        "lock screen",
        "verrouille l'écran",
        "loginctl lock-session",
    ),
    (
        "System/Session",
        "suspend computer",
        "mets en veille",
        "systemctl suspend",
    ),
    (
        "System/Network",
        "wifi on",
        "active le wifi",
        "nmcli radio wifi on",
    ),
    (
        "System/Network",
        "wifi off",
        "coupe le wifi",
        "nmcli radio wifi off",
    ),
    (
        "System/Network",
        "bluetooth on",
        "active le bluetooth",
        "bluetoothctl power on",
    ),
    (
        "System/Network",
        "bluetooth off",
        "coupe le bluetooth",
        "bluetoothctl power off",
    ),
    ("Desktop", "take screenshot", "capture d'écran", "grim"),
    (
        "Desktop/Notifications",
        "clear notifications",
        "efface les notifications",
        "makoctl dismiss --all",
    ),
    (
        "Desktop/Notifications",
        "do not disturb",
        "ne pas déranger",
        "makoctl mode -t do-not-disturb",
    ),
    (
        "Dicturs",
        "show commands",
        "montre les commandes",
        "dicturs menu",
    ),
    (
        "Dicturs",
        "manage commands",
        "gère les commandes",
        "dicturs menu --toggle",
    ),
];

impl Default for Config {
    /// Fresh setup: English samples, disabled, to be turned on one by one.
    fn default() -> Self {
        Self::seeded(Some("en"))
    }
}

/// Dicturs directories, after moving the ones of its former name (Voice Commander) if needed.
pub fn project_dirs() -> Result<ProjectDirs> {
    let dirs = ProjectDirs::from("", "", "dicturs")
        .ok_or_else(|| anyhow!("cannot determine home directory"))?;
    if let Some(old) = ProjectDirs::from("", "", "voice-commander") {
        for (from, to) in [
            (old.config_dir(), dirs.config_dir()),
            (old.data_dir(), dirs.data_dir()),
        ] {
            if from.is_dir() && !to.exists() {
                if let Some(parent) = to.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::rename(from, to);
            }
        }
    }
    Ok(dirs)
}

impl Config {
    pub fn dir() -> Result<PathBuf> {
        if let Ok(dir) = std::env::var("DICTURS_CONFIG_DIR") {
            return Ok(PathBuf::from(dir));
        }
        project_dirs().map(|d| d.config_dir().to_path_buf())
    }

    pub fn path() -> Result<PathBuf> {
        Ok(Self::dir()?.join("config.toml"))
    }

    /// A config with default settings, the built-in commands (enabled) and
    /// samples (disabled) in `lang`; `None` = built-ins only, in English.
    pub fn seeded(lang: Option<&str>) -> Self {
        let mut cfg = Self {
            settings: Settings::default(),
            categories: Vec::new(),
            commands: Vec::new(),
        };
        let lang_code = sample_language(lang.unwrap_or("en"));
        cfg.add_builtins(&lang_code);
        if lang.is_some() {
            cfg.add_samples(&lang_code);
        }
        cfg
    }

    /// True before the first save (first launch).
    pub fn is_fresh() -> Result<bool> {
        Ok(!Self::path()?.exists())
    }

    /// Loads the config; on first launch returns the defaults without writing them,
    /// so the user can still pick the samples language (`dicturs init`, TUI welcome).
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    // ---------- categories ----------

    /// Ensures a category and all its parents exist.
    pub fn ensure_category(&mut self, path: &str) -> Result<String> {
        let path = normalize_category(path)?;
        if path.is_empty() {
            return Ok(path);
        }
        let mut acc = String::new();
        for part in path.split(CATEGORY_SEP) {
            if !acc.is_empty() {
                acc.push(CATEGORY_SEP);
            }
            acc.push_str(part);
            if !self.categories.iter().any(|c| c.path == acc) {
                self.categories.push(Category {
                    path: acc.clone(),
                    enabled: true,
                });
            }
        }
        self.categories.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(path)
    }

    pub fn category(&self, path: &str) -> Option<&Category> {
        self.categories.iter().find(|c| c.path == path)
    }

    /// A category is active when it and all its ancestors are enabled.
    pub fn category_active(&self, path: &str) -> bool {
        if path.is_empty() {
            return true;
        }
        let mut acc = String::new();
        for part in path.split(CATEGORY_SEP) {
            if !acc.is_empty() {
                acc.push(CATEGORY_SEP);
            }
            acc.push_str(part);
            if self.category(&acc).is_some_and(|c| !c.enabled) {
                return false;
            }
        }
        true
    }

    pub fn toggle_category(&mut self, path: &str) -> Result<bool> {
        let cat = self
            .categories
            .iter_mut()
            .find(|c| c.path == path)
            .ok_or_else(|| anyhow!("unknown category '{path}'"))?;
        cat.enabled = !cat.enabled;
        Ok(cat.enabled)
    }

    pub fn set_category_enabled(&mut self, path: &str, enabled: bool) -> Result<()> {
        let cat = self
            .categories
            .iter_mut()
            .find(|c| c.path == path)
            .ok_or_else(|| anyhow!("unknown category '{path}'"))?;
        cat.enabled = enabled;
        Ok(())
    }

    /// Renames a category (and its subcategories / commands).
    pub fn rename_category(&mut self, from: &str, to: &str) -> Result<()> {
        let to = normalize_category(to)?;
        if to.is_empty() {
            bail!("new name cannot be empty");
        }
        if self.category(from).is_none() {
            bail!("unknown category '{from}'");
        }
        let remap = |p: &str| -> Option<String> {
            if p == from {
                Some(to.clone())
            } else {
                p.strip_prefix(from)
                    .and_then(|rest| rest.strip_prefix(CATEGORY_SEP))
                    .map(|rest| format!("{to}{CATEGORY_SEP}{rest}"))
            }
        };
        for c in &mut self.categories {
            if let Some(p) = remap(&c.path) {
                c.path = p;
            }
        }
        for c in &mut self.commands {
            if let Some(p) = remap(&c.category) {
                c.category = p;
            }
        }
        self.categories.dedup_by(|a, b| a.path == b.path);
        self.ensure_category(&to)?;
        Ok(())
    }

    /// Removes a category and its subcategories. Commands are moved to the parent.
    pub fn remove_category(&mut self, path: &str) -> Result<()> {
        if self.category(path).is_none() {
            bail!("unknown category '{path}'");
        }
        let parent = path
            .rsplit_once(CATEGORY_SEP)
            .map(|(p, _)| p)
            .unwrap_or("")
            .to_string();
        let inside = |p: &str| p == path || p.starts_with(&format!("{path}{CATEGORY_SEP}"));
        self.categories.retain(|c| !inside(&c.path));
        for c in &mut self.commands {
            if inside(&c.category) {
                c.category = parent.clone();
            }
        }
        Ok(())
    }

    // ---------- commands ----------

    fn next_id(&self) -> u32 {
        self.commands.iter().map(|c| c.id).max().unwrap_or(0) + 1
    }

    pub fn add_command(
        &mut self,
        keyword: &str,
        command: &str,
        category: &str,
        enabled: bool,
    ) -> Result<u32> {
        let keyword = keyword.trim();
        if keyword.is_empty() {
            bail!("keyword cannot be empty");
        }
        if command.trim().is_empty() {
            bail!("command cannot be empty");
        }
        let category = self.ensure_category(category)?;
        let id = self.next_id();
        self.commands.push(VoiceCommand {
            id,
            keyword: keyword.to_string(),
            command: command.trim().to_string(),
            category,
            enabled,
        });
        Ok(id)
    }

    /// Adds the built-in commands (`@…`) not configured yet, enabled. Returns how many were added.
    pub fn add_builtins(&mut self, language: &str) -> usize {
        let mut added = 0;
        for (en, fr, command) in crate::internal::BUILTINS {
            if self.commands.iter().any(|c| c.command == *command) {
                continue;
            }
            let keyword = if language == "fr" { fr } else { en };
            self.add_command(keyword, command, crate::internal::CATEGORY, true)
                .expect("valid built-in");
            added += 1;
        }
        added
    }

    /// Adds the samples whose command is not configured yet, disabled.
    /// Keywords are French for `fr`, English otherwise. Returns how many were added.
    pub fn add_samples(&mut self, language: &str) -> usize {
        let mut added = 0;
        for (category, en, fr, command) in SAMPLES {
            if self.commands.iter().any(|c| c.command == *command) {
                continue;
            }
            let keyword = if language == "fr" { fr } else { en };
            self.add_command(keyword, command, category, false)
                .expect("valid sample");
            added += 1;
        }
        added
    }

    pub fn command_mut(&mut self, id: u32) -> Result<&mut VoiceCommand> {
        self.commands
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| anyhow!("unknown command id {id}"))
    }

    pub fn update_command(
        &mut self,
        id: u32,
        keyword: Option<&str>,
        command: Option<&str>,
        category: Option<&str>,
    ) -> Result<()> {
        let category = category.map(|c| self.ensure_category(c)).transpose()?;
        let cmd = self.command_mut(id)?;
        if let Some(k) = keyword.map(str::trim).filter(|k| !k.is_empty()) {
            cmd.keyword = k.to_string();
        }
        if let Some(c) = command.map(str::trim).filter(|c| !c.is_empty()) {
            cmd.command = c.to_string();
        }
        if let Some(cat) = category {
            cmd.category = cat;
        }
        Ok(())
    }

    pub fn toggle_command(&mut self, id: u32) -> Result<bool> {
        let cmd = self.command_mut(id)?;
        cmd.enabled = !cmd.enabled;
        Ok(cmd.enabled)
    }

    pub fn remove_command(&mut self, id: u32) -> Result<VoiceCommand> {
        let idx = self
            .commands
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| anyhow!("unknown command id {id}"))?;
        Ok(self.commands.remove(idx))
    }

    /// Words given to the recognizer to bias its spelling: wake word + keywords.
    pub fn recognition_hints(&self) -> Vec<String> {
        let wake = self.settings.wake_word.trim();
        (!wake.is_empty())
            .then(|| wake.to_string())
            .into_iter()
            .chain(self.active_commands().map(|c| c.keyword.clone()))
            .collect()
    }

    /// Commands that can actually fire: enabled and in an active category.
    pub fn active_commands(&self) -> impl Iterator<Item = &VoiceCommand> {
        self.commands
            .iter()
            .filter(|c| c.enabled && self.category_active(&c.category))
    }

    /// Commands within a category (recursive when `recursive`), `""` = all.
    pub fn commands_in<'a>(
        &'a self,
        category: &'a str,
        recursive: bool,
    ) -> impl Iterator<Item = &'a VoiceCommand> + 'a {
        self.commands.iter().filter(move |c| {
            category.is_empty()
                || c.category == category
                || (recursive && c.category.starts_with(&format!("{category}{CATEGORY_SEP}")))
        })
    }

    // ---------- settings ----------

    pub const SETTING_KEYS: &'static [&'static str] = &[
        "engine",
        "whisper_model",
        "language",
        "threads",
        "external_command",
        "input_device",
        "vad_threshold",
        "silence_ms",
        "max_utterance_ms",
        "match_threshold",
        "wake_word",
        "launcher",
        "shell",
        "listen_on_start",
    ];

    pub fn get_setting(&self, key: &str) -> Result<String> {
        let s = &self.settings;
        Ok(match key {
            "engine" => s.engine.to_string(),
            "whisper_model" => s.whisper_model.clone(),
            "language" => s.language.clone(),
            "threads" => s.threads.to_string(),
            "external_command" => s.external_command.clone(),
            "input_device" => s.input_device.clone(),
            "vad_threshold" => s.vad_threshold.to_string(),
            "silence_ms" => s.silence_ms.to_string(),
            "max_utterance_ms" => s.max_utterance_ms.to_string(),
            "match_threshold" => s.match_threshold.to_string(),
            "wake_word" => s.wake_word.clone(),
            "launcher" => s.launcher.clone(),
            "shell" => s.shell.clone(),
            "listen_on_start" => on_off(s.listen_on_start).into(),
            _ => bail!(
                "unknown setting '{key}' (valid: {})",
                Self::SETTING_KEYS.join(", ")
            ),
        })
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        let s = &mut self.settings;
        let v = value.trim();
        match key {
            "engine" => {
                s.engine = match v {
                    "whisper" => EngineKind::Whisper,
                    "external" => EngineKind::External,
                    _ => bail!("engine must be 'whisper' or 'external'"),
                }
            }
            "whisper_model" => {
                if crate::models::find(v).is_none() {
                    bail!("unknown model '{v}', see `dicturs model list`");
                }
                s.whisper_model = v.into()
            }
            "language" => {
                s.language = if v.is_empty() {
                    "system".into()
                } else {
                    v.to_lowercase()
                }
            }
            "threads" => s.threads = v.parse().context("threads must be an integer")?,
            "external_command" => s.external_command = v.into(),
            "input_device" => s.input_device = v.into(),
            "vad_threshold" => s.vad_threshold = parse_range(v, 0.0, 1.0)? as f32,
            "silence_ms" => s.silence_ms = v.parse().context("silence_ms must be an integer")?,
            "max_utterance_ms" => {
                s.max_utterance_ms = v.parse().context("max_utterance_ms must be an integer")?
            }
            "match_threshold" => s.match_threshold = parse_range(v, 0.0, 1.0)?,
            "wake_word" => s.wake_word = v.into(),
            "launcher" => s.launcher = v.into(),
            "shell" => {
                s.shell = if v.is_empty() {
                    "auto".into()
                } else {
                    v.into()
                }
            }
            "listen_on_start" => s.listen_on_start = parse_bool(v)?,
            _ => bail!(
                "unknown setting '{key}' (valid: {})",
                Self::SETTING_KEYS.join(", ")
            ),
        }
        Ok(())
    }
}

impl Settings {
    /// Language actually given to the recognizer (`system` resolved from the locale).
    pub fn effective_language(&self) -> String {
        match self.language.as_str() {
            "system" => system_language().unwrap_or_else(|| "auto".into()),
            other => other.to_string(),
        }
    }
}

/// Samples exist in English and French: resolves `system` and falls back to English.
pub fn sample_language(lang: &str) -> String {
    let lang = match lang.trim().to_lowercase().as_str() {
        "system" => system_language().unwrap_or_default(),
        other => other.to_string(),
    };
    if lang == "fr" { lang } else { "en".into() }
}

/// Language code from the user locale (`LC_ALL` > `LC_MESSAGES` > `LANG` > `LANGUAGE`),
/// e.g. `fr_FR.UTF-8` -> `fr`. `C`/`POSIX` locales give `None`.
pub fn system_language() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .and_then(|v| language_from_locale(&v))
}

fn language_from_locale(locale: &str) -> Option<String> {
    let code: String = locale
        .split(':')
        .next()?
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_lowercase();
    (code.len() >= 2 && code != "c" && code != "posix").then_some(code)
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

fn parse_bool(v: &str) -> Result<bool> {
    match v.to_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Ok(true),
        "off" | "false" | "no" | "0" => Ok(false),
        _ => bail!("expected on/off"),
    }
}

fn parse_range(v: &str, min: f64, max: f64) -> Result<f64> {
    let x: f64 = v.parse().context("expected a number")?;
    if !(min..=max).contains(&x) {
        bail!("value must be between {min} and {max}");
    }
    Ok(x)
}

/// Trims each path segment and drops empty ones: ` a / /b ` -> `a/b`.
pub fn normalize_category(path: &str) -> Result<String> {
    let parts: Vec<&str> = path
        .split(CATEGORY_SEP)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    Ok(parts.join(&CATEGORY_SEP.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Config {
        Config {
            settings: Settings::default(),
            categories: vec![],
            commands: vec![],
        }
    }

    #[test]
    fn subcategories_are_created_and_inherit_disable() {
        let mut c = empty();
        let id = c
            .add_command("play", "playerctl play", "Media/Music", true)
            .unwrap();
        assert!(c.category("Media").is_some());
        assert!(c.category("Media/Music").is_some());
        assert_eq!(c.active_commands().count(), 1);
        c.toggle_category("Media").unwrap();
        assert_eq!(c.active_commands().count(), 0);
        c.toggle_category("Media").unwrap();
        c.toggle_command(id).unwrap();
        assert_eq!(c.active_commands().count(), 0);
    }

    #[test]
    fn fresh_setup_has_disabled_samples() {
        let mut c = empty();
        assert_eq!(c.add_samples("fr"), SAMPLES.len());
        assert!(c.commands.iter().all(|c| !c.enabled));
        assert_eq!(c.active_commands().count(), 0);
        // Built-ins are on by default.
        let seeded = Config::seeded(Some("en"));
        assert_eq!(
            seeded.active_commands().count(),
            crate::internal::BUILTINS.len()
        );
        assert!(c.commands.iter().any(|c| c.keyword == "monte le son"));
        // Importing again adds nothing.
        assert_eq!(c.add_samples("en"), 0);
    }

    #[test]
    fn locale_parsing() {
        assert_eq!(language_from_locale("fr_FR.UTF-8").as_deref(), Some("fr"));
        assert_eq!(language_from_locale("en_US:en").as_deref(), Some("en"));
        assert_eq!(language_from_locale("C.UTF-8"), None);
        assert_eq!(language_from_locale("POSIX"), None);
    }

    #[test]
    fn rename_and_remove_category() {
        let mut c = empty();
        c.add_command("a", "true", "X/Y", true).unwrap();
        c.rename_category("X", "Z").unwrap();
        assert_eq!(c.commands[0].category, "Z/Y");
        assert!(c.category("X").is_none());
        c.remove_category("Z/Y").unwrap();
        assert_eq!(c.commands[0].category, "Z");
    }
}

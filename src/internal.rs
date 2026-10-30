//! Built-in commands: entries whose command starts with `@`, handled by dicturs itself.

use anyhow::{Result, bail};

use crate::config::Config;
use crate::matcher::normalize;
use crate::models;

/// Built-in commands: (English keyword, French keyword, command). Enabled by default.
pub const BUILTINS: &[(&str, &str, &str)] = &[
    ("stop listening", "arrête d'écouter", "@stop-listening"),
    ("change model", "change de modèle", "@model {args}"),
    ("change language", "change de langue", "@language {args}"),
    ("dry run", "mode test", "@dry-run"),
];

pub const CATEGORY: &str = "Dicturs";

pub fn is_internal(command: &str) -> bool {
    command.trim_start().starts_with('@')
}

pub enum Action {
    StopListening,
    ToggleDryRun,
    SetModel(String),
    SetLanguage(String),
}

/// What the caller (TUI or `dicturs listen`) must do after a built-in ran.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    StopListening,
    ToggleDryRun,
    /// Engine settings changed: restart listening to apply them.
    RestartListening,
}

/// Parses an expanded built-in command, e.g. `@model small`.
pub fn parse(command: &str) -> Result<Action> {
    let command = command.trim().trim_start_matches('@');
    let (name, args) = command.split_once(' ').unwrap_or((command, ""));
    Ok(match name {
        "stop-listening" | "stop" => Action::StopListening,
        "dry-run" => Action::ToggleDryRun,
        "model" => Action::SetModel(resolve_model(args)?),
        "language" => Action::SetLanguage(resolve_language(args)?),
        other => bail!("unknown built-in command @{other}"),
    })
}

/// Runs a built-in: settings are changed and saved here, the rest is left to the caller.
/// Returns the effect and a message for the user.
pub fn run(cfg: &mut Config, command: &str) -> Result<(Effect, String)> {
    Ok(match parse(command)? {
        Action::StopListening => (Effect::StopListening, "stop listening".into()),
        Action::ToggleDryRun => (Effect::ToggleDryRun, "toggle dry-run".into()),
        Action::SetModel(m) => {
            if !models::is_installed(&m) {
                bail!("model {m} is not downloaded (dicturs model download {m})");
            }
            cfg.set_setting("whisper_model", &m)?;
            cfg.save()?;
            (Effect::RestartListening, format!("model set to {m}"))
        }
        Action::SetLanguage(l) => {
            cfg.set_setting("language", &l)?;
            cfg.save()?;
            (
                Effect::RestartListening,
                format!(
                    "language set to {l} ({})",
                    cfg.settings.effective_language()
                ),
            )
        }
    })
}

/// Maps spoken words to a model name: "small", "petit", "turbo", "large v3"...
fn resolve_model(spoken: &str) -> Result<String> {
    let s = normalize(spoken);
    if s.is_empty() {
        bail!("which model? e.g. “change model small”");
    }
    if let Some(m) = models::MODELS.iter().find(|m| normalize(m.name) == s) {
        return Ok(m.name.to_string());
    }
    let has = |words: &[&str]| s.split(' ').any(|w| words.contains(&w));
    let name = if has(&["turbo"]) {
        "large-v3-turbo"
    } else if has(&["large", "grand", "gros"]) {
        "large-v3"
    } else if has(&["medium", "moyen"]) {
        "medium"
    } else if has(&["small", "petit"]) {
        "small"
    } else if has(&["base", "basique", "normal"]) {
        "base"
    } else if has(&["tiny", "minuscule", "mini"]) {
        "tiny"
    } else {
        bail!("unknown model “{spoken}”");
    };
    Ok(name.to_string())
}

/// Maps spoken words to a language setting: "french", "anglais", "system", "fr"...
fn resolve_language(spoken: &str) -> Result<String> {
    const NAMES: &[(&str, &[&str])] = &[
        ("system", &["system", "systeme"]),
        ("auto", &["auto", "automatic", "automatique"]),
        ("en", &["en", "english", "anglais"]),
        ("fr", &["fr", "french", "francais"]),
        ("de", &["de", "german", "allemand", "deutsch"]),
        ("es", &["es", "spanish", "espagnol", "espanol"]),
        ("it", &["it", "italian", "italien", "italiano"]),
        ("pt", &["pt", "portuguese", "portugais"]),
        ("nl", &["nl", "dutch", "neerlandais"]),
    ];
    let s = normalize(spoken);
    if s.is_empty() {
        bail!("which language? e.g. “change language french”");
    }
    // Full names first: in “en français”, “en” is a French word, not the code.
    let lookup = |full: bool| {
        s.split(' ')
            .filter(|w| (w.len() > 2) == full)
            .find_map(|w| {
                NAMES
                    .iter()
                    .find(|(_, names)| names.contains(&w))
                    .map(|(code, _)| code.to_string())
            })
    };
    lookup(true)
        .or_else(|| lookup(false))
        .ok_or_else(|| anyhow::anyhow!("unknown language “{spoken}”"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_builtins() {
        assert!(matches!(
            parse("@stop-listening").unwrap(),
            Action::StopListening
        ));
        assert!(matches!(parse("@model Petit").unwrap(), Action::SetModel(m) if m == "small"));
        assert!(
            matches!(parse("@model large v3 turbo").unwrap(), Action::SetModel(m) if m == "large-v3-turbo")
        );
        assert!(
            matches!(parse("@language en français").unwrap(), Action::SetLanguage(l) if l == "fr")
        );
        assert!(parse("@model").is_err());
        assert!(parse("@language klingon").is_err());
        assert!(parse("@nope").is_err());
    }
}

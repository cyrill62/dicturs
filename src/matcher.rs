use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::config::{Config, VoiceCommand};

#[derive(Debug, Clone)]
pub struct Match<'a> {
    pub command: &'a VoiceCommand,
    pub score: f64,
    /// Words spoken after the keyword (letters, digits and spaces only),
    /// filled when the command uses `{args}`.
    pub args: String,
}

/// Lowercases, removes accents and punctuation: `Ouvre  le Navigateur !` -> `ouvre le navigateur`.
pub fn normalize(text: &str) -> String {
    let folded: String = text
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Words of a transcription, both as spoken (for arguments) and normalized (for matching).
#[derive(Debug, Clone, Default)]
struct Tokens {
    spoken: Vec<String>,
    norm: Vec<String>,
}

impl Tokens {
    /// Splits on anything that is not alphanumeric, so tokens never contain
    /// punctuation or shell metacharacters.
    fn new(text: &str) -> Self {
        let mut t = Tokens::default();
        for word in text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
        {
            let norm = normalize(word);
            if !norm.is_empty() {
                t.spoken.push(word.to_string());
                t.norm.push(norm);
            }
        }
        t
    }

    fn without(&self, start: usize, end: usize) -> Self {
        let cut = |v: &Vec<String>| v[..start].iter().chain(&v[end..]).cloned().collect();
        Tokens {
            spoken: cut(&self.spoken),
            norm: cut(&self.norm),
        }
    }
}

/// Best fuzzy score (0..1) of `keyword` (normalized) in `words`,
/// with the index right after the matching window.
fn score_at(words: &[String], keyword: &str) -> (f64, usize) {
    if keyword.is_empty() || words.is_empty() {
        return (0.0, 0);
    }
    let n = keyword.split(' ').count();
    if words.len() < n {
        return (
            strsim::normalized_levenshtein(&words.join(" "), keyword),
            words.len(),
        );
    }
    let mut best = (0.0f64, 0usize, usize::MAX);
    for len in n.saturating_sub(1).max(1)..=n + 1 {
        if len > words.len() {
            break;
        }
        for (i, window) in words.windows(len).enumerate() {
            let s = strsim::normalized_levenshtein(&window.join(" "), keyword);
            // On ties prefer the window with the keyword's own length.
            let size_gap = len.abs_diff(n);
            if s > best.0 || (s == best.0 && size_gap < best.2) {
                best = (s, i + len, size_gap);
            }
        }
    }
    (best.0, best.1)
}

/// Best fuzzy score (0..1) of `keyword` appearing somewhere in `text` (both normalized).
#[cfg(test)]
fn score(text: &str, keyword: &str) -> f64 {
    let words: Vec<String> = text.split(' ').map(str::to_string).collect();
    score_at(&words, keyword).0
}

/// Minimal similarity for the wake word. Names like "Jarvis" are often spelled
/// differently by the recognizer ("Jarvisse", "Charvis"), so it is more tolerant
/// than command matching.
const WAKE_THRESHOLD: f64 = 0.65;

/// Removes the wake word (anywhere in the tokens). Returns `None` when it is missing.
fn strip_wake_word(tokens: &Tokens, wake: &str) -> Option<Tokens> {
    if wake.is_empty() {
        return Some(tokens.clone());
    }
    let n = wake.split(' ').count();
    let similar = |candidate: &str| {
        candidate.starts_with(wake)
            || strsim::normalized_levenshtein(candidate, wake) >= WAKE_THRESHOLD
    };
    (0..tokens.norm.len().saturating_sub(n - 1))
        .find(|&i| similar(&tokens.norm[i..i + n].join(" ")))
        .map(|i| tokens.without(i, i + n))
}

/// Whether a command consumes the words following its keyword.
pub fn takes_args(command: &str) -> bool {
    command.contains("{args}") || command.contains("{args_url}")
}

/// Substitutes `{args}` and `{args_url}` in a command.
pub fn expand(command: &str, args: &str) -> String {
    command
        .replace("{args_url}", &url_encode(args))
        .replace("{args}", args)
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Why a transcription did or did not trigger a command.
pub enum Outcome<'a> {
    Matched(Match<'a>),
    /// The wake word is configured but was not heard.
    NoWakeWord,
    NoActiveCommand,
    /// Best candidate, below the threshold.
    BelowThreshold(Match<'a>),
}

pub fn evaluate<'a>(cfg: &'a Config, transcription: &str) -> Outcome<'a> {
    let threshold = cfg.settings.match_threshold;
    let Some(tokens) = strip_wake_word(
        &Tokens::new(transcription),
        &normalize(&cfg.settings.wake_word),
    ) else {
        return Outcome::NoWakeWord;
    };
    let best = cfg
        .active_commands()
        .map(|c| {
            let (score, end) = score_at(&tokens.norm, &normalize(&c.keyword));
            (c, score, end)
        })
        .max_by(|a, b| {
            a.1.total_cmp(&b.1)
                .then(a.0.keyword.len().cmp(&b.0.keyword.len()))
        });
    let Some((command, score, end)) = best else {
        return Outcome::NoActiveCommand;
    };
    let args = if takes_args(&command.command) {
        tokens.spoken[end..].join(" ")
    } else {
        String::new()
    };
    let m = Match {
        command,
        score,
        args,
    };
    if score >= threshold {
        Outcome::Matched(m)
    } else {
        Outcome::BelowThreshold(m)
    }
}

impl Outcome<'_> {
    /// Human readable reason for a non-match.
    pub fn reason(&self, cfg: &Config) -> String {
        match self {
            Outcome::Matched(m) => format!("matched “{}”", m.command.keyword),
            Outcome::NoWakeWord => {
                format!("no match: wake word “{}” not heard", cfg.settings.wake_word)
            }
            Outcome::NoActiveCommand => "no match: no active command".into(),
            Outcome::BelowThreshold(m) => format!(
                "no match: closest is “{}” at {:.0}% (threshold {:.0}%)",
                m.command.keyword,
                m.score * 100.0,
                cfg.settings.match_threshold * 100.0
            ),
        }
    }
}

/// Finds the best active command for a transcription.
#[cfg(test)]
pub fn find_match<'a>(cfg: &'a Config, transcription: &str) -> Option<Match<'a>> {
    match evaluate(cfg, transcription) {
        Outcome::Matched(m) => Some(m),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_accents_and_punctuation() {
        assert_eq!(
            normalize("  Ouvre le Navigateur, s'il te plaît !"),
            "ouvre le navigateur s il te plait"
        );
    }

    #[test]
    fn scores() {
        assert_eq!(score("please open browser now", "open browser"), 1.0);
        assert!(score("open browsers", "open browser") > 0.9);
        assert!(score("hello world", "volume up") < 0.5);
    }

    fn strip(text: &str, wake: &str) -> Option<String> {
        strip_wake_word(&Tokens::new(text), wake).map(|t| t.norm.join(" "))
    }

    #[test]
    fn wake_word() {
        assert_eq!(
            strip("hey jarvis mute", "jarvis").as_deref(),
            Some("hey mute")
        );
        assert_eq!(
            strip("volume up jarvis", "jarvis").as_deref(),
            Some("volume up")
        );
        assert_eq!(
            strip("jarvisse volume up", "jarvis").as_deref(),
            Some("volume up")
        );
        assert_eq!(
            strip("charvis volume up", "jarvis").as_deref(),
            Some("volume up")
        );
        assert_eq!(strip("mute", "jarvis"), None);
        assert_eq!(strip("volume up", "jarvis"), None);
    }

    fn cfg_with(commands: &[(&str, &str)]) -> Config {
        let mut cfg = Config::seeded(None);
        cfg.commands.clear();
        for (k, c) in commands {
            cfg.add_command(k, c, "", true).unwrap();
        }
        cfg
    }

    #[test]
    fn prefers_longest_exact_keyword() {
        let cfg = cfg_with(&[("volume", "a"), ("volume up", "b")]);
        let m = find_match(&cfg, "Volume up!").unwrap();
        assert_eq!(m.command.command, "b");
    }

    #[test]
    fn args_are_the_words_after_the_keyword() {
        let cfg = cfg_with(&[("search for", "open {args_url}"), ("mute", "m")]);
        let m = find_match(&cfg, "Jarvis, search for Café crème; rm -rf $(ls)!").unwrap();
        assert_eq!(m.args, "Café crème rm rf ls");
        assert_eq!(
            expand(&m.command.command, &m.args),
            "open Caf%C3%A9%20cr%C3%A8me%20rm%20rf%20ls"
        );
        // Commands without {args} ignore trailing words.
        assert_eq!(find_match(&cfg, "mute please").unwrap().args, "");
    }
}

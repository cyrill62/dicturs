//! Speech-to-text engines.

use anyhow::{Context, Result, bail};

use crate::audio;
use crate::config::{EngineKind, Settings};

pub trait Recognizer: Send {
    /// Transcribes 16 kHz mono samples.
    fn transcribe(&mut self, samples: &[f32]) -> Result<String>;
}

/// Builds the configured engine. `hints` (keywords) bias recognition when supported.
pub fn build(settings: &Settings, hints: &[String]) -> Result<Box<dyn Recognizer>> {
    match settings.engine {
        EngineKind::Whisper => whisper(settings, hints),
        EngineKind::External => Ok(Box::new(External {
            command: settings.external_command.clone(),
        })),
    }
}

#[cfg(feature = "whisper")]
fn whisper(settings: &Settings, hints: &[String]) -> Result<Box<dyn Recognizer>> {
    Ok(Box::new(whisper_engine::Whisper::new(settings, hints)?))
}

#[cfg(not(feature = "whisper"))]
fn whisper(_: &Settings, _: &[String]) -> Result<Box<dyn Recognizer>> {
    bail!("built without the `whisper` feature: use engine = external")
}

/// Removes whisper artefacts such as `[BLANK_AUDIO]`, `(music)` or timestamps.
pub fn clean_transcript(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(feature = "whisper")]
mod whisper_engine {
    use super::*;
    use whisper_rs::{
        FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
    };

    pub struct Whisper {
        state: WhisperState,
        language: String,
        threads: i32,
        prompt: String,
    }

    impl Whisper {
        pub fn new(settings: &Settings, hints: &[String]) -> Result<Self> {
            whisper_rs::install_logging_hooks(); // silences whisper.cpp output
            let path = crate::models::model_path(&settings.whisper_model)?;
            if !path.is_file() {
                bail!(
                    "model '{}' not downloaded: run `dicturs model download {}`",
                    settings.whisper_model,
                    settings.whisper_model
                );
            }
            let ctx = WhisperContext::new_with_params(
                path.to_str().context("non UTF-8 model path")?,
                WhisperContextParameters::default(),
            )
            .context("loading whisper model")?;
            let state = ctx.create_state().context("creating whisper state")?;
            Ok(Self {
                state,
                language: settings.effective_language(),
                threads: settings.threads.max(1) as i32,
                prompt: hints.join(", "),
            })
        }
    }

    impl Recognizer for Whisper {
        fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_language(Some(&self.language));
            params.set_n_threads(self.threads);
            params.set_no_context(true);
            params.set_single_segment(true);
            params.set_no_timestamps(true);
            params.set_suppress_blank(true);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            if !self.prompt.is_empty() {
                params.set_initial_prompt(&self.prompt);
            }
            // whisper.cpp rejects inputs shorter than 1 s: pad with silence.
            let min = audio::SAMPLE_RATE as usize * 11 / 10;
            let mut audio = samples.to_vec();
            if audio.len() < min {
                audio.resize(min, 0.0);
            }
            self.state
                .full(params, &audio)
                .context("whisper inference")?;
            let text: String = self
                .state
                .as_iter()
                .map(|s| s.to_str_lossy().map(|t| t.into_owned()).unwrap_or_default())
                .collect();
            Ok(clean_transcript(&text))
        }
    }
}

/// Any program that takes a WAV file and prints the transcription on stdout.
struct External {
    command: String,
}

impl Recognizer for External {
    fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        let wav = std::env::temp_dir().join(format!("dicturs-{}.wav", std::process::id()));
        audio::write_wav(&wav, samples)?;
        let cmd = if self.command.contains("{wav}") {
            self.command.replace("{wav}", &wav.to_string_lossy())
        } else {
            format!("{} {}", self.command, wav.display())
        };
        let (shell, flag) = if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };
        let out = std::process::Command::new(shell)
            .arg(flag)
            .arg(&cmd)
            .output();
        let _ = std::fs::remove_file(&wav);
        let out = out.context("running external STT command")?;
        if !out.status.success() {
            bail!(
                "external STT failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(clean_transcript(&String::from_utf8_lossy(&out.stdout)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans() {
        assert_eq!(clean_transcript(" [BLANK_AUDIO] "), "");
        assert_eq!(
            clean_transcript("[00:00.000 --> 00:01.000]  Volume up. (music)"),
            "Volume up."
        );
    }
}

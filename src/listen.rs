//! Listening pipeline: microphone -> VAD -> recognizer -> transcription events.
//! Matching/execution is left to the consumer so it always uses the current config.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::audio::{self, AudioEvent, Capture};
use crate::config::Config;
use crate::stt;

pub enum ListenEvent {
    Status(String),
    Level(f32),
    SpeechStarted,
    Transcribing,
    Transcribed { text: String, took: Duration },
    Error(String),
}

pub struct Listener {
    capture: Option<Capture>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Listener {
    pub fn start(cfg: &Config, events: Sender<ListenEvent>) -> Result<Self> {
        let settings = cfg.settings.clone();
        let hints = cfg.recognition_hints();
        let _ = events.send(ListenEvent::Status(format!(
            "loading {} engine (language: {})…",
            settings.engine,
            settings.effective_language()
        )));
        let recognizer = stt::build(&settings, &hints)?;
        let (audio_tx, audio_rx) = mpsc::channel();
        let capture = audio::start(&settings, audio_tx)?;
        let _ = events.send(ListenEvent::Status("listening".into()));
        let worker = thread::spawn(move || run(recognizer, audio_rx, events));
        Ok(Self {
            capture: Some(capture),
            worker: Some(worker),
        })
    }

    pub fn stop(&mut self) {
        // Stopping the capture closes the audio channel, which ends the worker.
        drop(self.capture.take());
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(
    mut recognizer: Box<dyn stt::Recognizer>,
    audio: Receiver<AudioEvent>,
    events: Sender<ListenEvent>,
) {
    let mut last_level = Instant::now();
    for ev in audio {
        let out = match ev {
            AudioEvent::Level(l) => {
                // Throttle level updates to ~20/s.
                if last_level.elapsed() < Duration::from_millis(50) {
                    continue;
                }
                last_level = Instant::now();
                ListenEvent::Level(l)
            }
            AudioEvent::SpeechStarted => ListenEvent::SpeechStarted,
            AudioEvent::Error(e) => ListenEvent::Error(e),
            AudioEvent::Utterance(samples) => {
                let _ = events.send(ListenEvent::Transcribing);
                let start = Instant::now();
                match recognizer.transcribe(&samples) {
                    Ok(text) => ListenEvent::Transcribed {
                        text,
                        took: start.elapsed(),
                    },
                    Err(e) => ListenEvent::Error(format!("{e:#}")),
                }
            }
        };
        if events.send(out).is_err() {
            break;
        }
    }
}

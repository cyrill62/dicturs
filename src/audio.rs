//! Microphone capture (cpal) + energy based voice activity detection.
//! Produces 16 kHz mono f32 utterances, the format whisper expects.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};

use crate::config::Settings;

pub const SAMPLE_RATE: u32 = 16_000;
const FRAME: usize = (SAMPLE_RATE as usize) * 30 / 1000; // 30 ms
const PRE_ROLL_FRAMES: usize = 10; // 300 ms kept before speech onset
const MIN_SPEECH_FRAMES: usize = 8; // ignore clicks shorter than 240 ms

pub enum AudioEvent {
    /// Current input level (RMS, 0..1), sent every frame.
    Level(f32),
    SpeechStarted,
    Utterance(Vec<f32>),
    Error(String),
}

pub struct InputDevice {
    pub id: String,
    pub description: String,
    pub is_default: bool,
}

pub fn list_input_devices() -> Result<Vec<InputDevice>> {
    let host = cpal::default_host();
    let default_id = host
        .default_input_device()
        .and_then(|d| d.id().ok())
        .map(|i| i.to_string());
    let mut out = Vec::new();
    for dev in host.input_devices()? {
        let id = dev
            .id()
            .map(|i| i.to_string())
            .unwrap_or_else(|_| "?".into());
        let description = dev.description().map(|d| d.to_string()).unwrap_or_default();
        out.push(InputDevice {
            is_default: Some(&id) == default_id.as_ref(),
            id,
            description,
        });
    }
    Ok(out)
}

fn pick_device(name: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if name.is_empty() {
        return host
            .default_input_device()
            .ok_or_else(|| anyhow!("no default input device"));
    }
    host.input_devices()?
        .find(|d| {
            d.id().is_ok_and(|i| i.to_string() == name)
                || d.description()
                    .is_ok_and(|desc| desc.name() == name || desc.to_string() == name)
        })
        .ok_or_else(|| anyhow!("input device '{name}' not found (see `dicturs devices`)"))
}

/// Running capture; dropping it stops the threads.
pub struct Capture {
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}

impl Capture {
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn start(settings: &Settings, events: Sender<AudioEvent>) -> Result<Capture> {
    let stop = Arc::new(AtomicBool::new(false));
    let (raw_tx, raw_rx) = mpsc::channel::<Vec<f32>>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32>>();

    // The cpal stream is not `Send`: build and keep it inside its own thread.
    let device_name = settings.input_device.clone();
    let stop_audio = stop.clone();
    let audio_events = events.clone();
    let audio = thread::spawn(move || {
        let stream = match build_stream(&device_name, raw_tx, audio_events) {
            Ok((stream, rate)) => {
                let _ = ready_tx.send(Ok(rate));
                stream
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        while !stop_audio.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(50));
        }
        drop(stream);
    });

    let input_rate = ready_rx
        .recv()
        .map_err(|_| anyhow!("audio thread died"))
        .and_then(|r| r)?;

    let vad = Vad::new(settings);
    let stop_vad = stop.clone();
    let processor = thread::spawn(move || process(raw_rx, input_rate, vad, events, stop_vad));

    Ok(Capture {
        stop,
        threads: vec![audio, processor],
    })
}

fn build_stream(
    device_name: &str,
    raw_tx: Sender<Vec<f32>>,
    events: Sender<AudioEvent>,
) -> Result<(cpal::Stream, u32)> {
    let device = pick_device(device_name)?;
    let supported = device
        .default_input_config()
        .context("querying input config")?;
    let rate = supported.sample_rate();
    let channels = supported.channels() as usize;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    fn build<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        channels: usize,
        tx: Sender<Vec<f32>>,
        events: Sender<AudioEvent>,
    ) -> Result<cpal::Stream>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let stream = device.build_input_stream(
            *config,
            move |data: &[T], _: &_| {
                let mono: Vec<f32> = data
                    .chunks(channels)
                    .map(|frame| {
                        frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / channels as f32
                    })
                    .collect();
                let _ = tx.send(mono);
            },
            move |err| {
                let _ = events.send(AudioEvent::Error(format!("audio stream: {err}")));
            },
            None,
        )?;
        Ok(stream)
    }

    let stream = match format {
        SampleFormat::I8 => build::<i8>(&device, &config, channels, raw_tx, events)?,
        SampleFormat::I16 => build::<i16>(&device, &config, channels, raw_tx, events)?,
        SampleFormat::I32 => build::<i32>(&device, &config, channels, raw_tx, events)?,
        SampleFormat::U8 => build::<u8>(&device, &config, channels, raw_tx, events)?,
        SampleFormat::U16 => build::<u16>(&device, &config, channels, raw_tx, events)?,
        SampleFormat::F32 => build::<f32>(&device, &config, channels, raw_tx, events)?,
        SampleFormat::F64 => build::<f64>(&device, &config, channels, raw_tx, events)?,
        other => bail!("unsupported sample format {other}"),
    };
    stream.play()?;
    Ok((stream, rate))
}

/// Streaming linear resampler to 16 kHz.
struct Resampler {
    step: f64,
    pos: f64,
    last: f32,
}

impl Resampler {
    fn new(input_rate: u32) -> Self {
        Self {
            step: input_rate as f64 / SAMPLE_RATE as f64,
            pos: -1.0,
            last: 0.0,
        }
    }

    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        // Positions index `input`, with -1 standing for the last sample of the previous chunk.
        let last = self.last;
        let at = |i: isize| if i < 0 { last } else { input[i as usize] };
        let end = input.len() as f64 - 1.0;
        while self.pos < end {
            let i = self.pos.floor() as isize;
            let frac = (self.pos - i as f64) as f32;
            let (a, b) = (at(i), at(i + 1));
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        self.pos -= input.len() as f64;
        if let Some(&l) = input.last() {
            self.last = l;
        }
    }
}

struct Vad {
    threshold: f32,
    silence_frames: usize,
    max_frames: usize,
    pre_roll: VecDeque<Vec<f32>>,
    current: Option<Utterance>,
}

struct Utterance {
    samples: Vec<f32>,
    speech_frames: usize,
    silent_run: usize,
    frames: usize,
}

impl Vad {
    fn new(s: &Settings) -> Self {
        Self {
            threshold: s.vad_threshold,
            silence_frames: (s.silence_ms as usize / 30).max(1),
            max_frames: (s.max_utterance_ms as usize / 30).max(10),
            pre_roll: VecDeque::new(),
            current: None,
        }
    }

    /// Feeds one 30 ms frame; returns a finished utterance if any.
    fn feed(&mut self, frame: Vec<f32>, rms: f32, events: &Sender<AudioEvent>) -> Option<Vec<f32>> {
        let speech = rms >= self.threshold;
        match &mut self.current {
            None => {
                if speech {
                    let mut samples: Vec<f32> = self.pre_roll.drain(..).flatten().collect();
                    samples.extend_from_slice(&frame);
                    self.current = Some(Utterance {
                        samples,
                        speech_frames: 1,
                        silent_run: 0,
                        frames: 1,
                    });
                    let _ = events.send(AudioEvent::SpeechStarted);
                } else {
                    self.pre_roll.push_back(frame);
                    if self.pre_roll.len() > PRE_ROLL_FRAMES {
                        self.pre_roll.pop_front();
                    }
                }
                None
            }
            Some(u) => {
                u.samples.extend_from_slice(&frame);
                u.frames += 1;
                if speech {
                    u.speech_frames += 1;
                    u.silent_run = 0;
                } else {
                    u.silent_run += 1;
                }
                if u.silent_run >= self.silence_frames || u.frames >= self.max_frames {
                    let u = self.current.take().expect("current utterance");
                    (u.speech_frames >= MIN_SPEECH_FRAMES).then_some(u.samples)
                } else {
                    None
                }
            }
        }
    }
}

fn process(
    raw_rx: Receiver<Vec<f32>>,
    input_rate: u32,
    mut vad: Vad,
    events: Sender<AudioEvent>,
    stop: Arc<AtomicBool>,
) {
    let mut resampler = Resampler::new(input_rate);
    let mut pending: Vec<f32> = Vec::with_capacity(FRAME * 4);
    while !stop.load(Ordering::SeqCst) {
        let chunk = match raw_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(c) => c,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        resampler.push(&chunk, &mut pending);
        while pending.len() >= FRAME {
            let frame: Vec<f32> = pending.drain(..FRAME).collect();
            let rms = (frame.iter().map(|s| s * s).sum::<f32>() / FRAME as f32).sqrt();
            if events.send(AudioEvent::Level(rms)).is_err() {
                return;
            }
            if let Some(utterance) = vad.feed(frame, rms, &events)
                && events.send(AudioEvent::Utterance(utterance)).is_err()
            {
                return;
            }
        }
    }
}

/// Decodes any audio file to 16 kHz mono f32 samples using ffmpeg.
pub fn decode_file(path: &std::path::Path) -> Result<Vec<f32>> {
    let out = std::process::Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-f", "f32le", "-ac", "1", "-ar", "16000", "-"])
        .output()
        .context("running ffmpeg (is it installed?)")?;
    if !out.status.success() {
        bail!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out
        .stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

/// Writes 16 kHz mono f32 samples as a 16-bit PCM WAV file.
pub fn write_wav(path: &std::path::Path, samples: &[f32]) -> Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    buf.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        buf.extend_from_slice(&i16::from_sample(*s).to_le_bytes());
    }
    std::fs::write(path, buf)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_ratio() {
        let mut r = Resampler::new(48_000);
        let mut out = Vec::new();
        for _ in 0..10 {
            r.push(&vec![0.5; 4800], &mut out);
        }
        assert!((out.len() as i64 - 16_000).abs() <= 1, "{}", out.len());
        assert!(out.iter().skip(1).all(|s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn vad_segments_speech() {
        let (tx, _rx) = mpsc::channel();
        let mut vad = Vad::new(&Settings {
            silence_ms: 300,
            ..Settings::default()
        });
        let loud = vec![0.3; FRAME];
        let quiet = vec![0.0; FRAME];
        let mut done = None;
        for _ in 0..20 {
            assert!(vad.feed(loud.clone(), 0.3, &tx).is_none());
        }
        for _ in 0..20 {
            if let Some(u) = vad.feed(quiet.clone(), 0.0, &tx) {
                done = Some(u);
                break;
            }
        }
        assert!(done.unwrap().len() >= 20 * FRAME);
    }
}

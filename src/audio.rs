use anyhow::{Context, Result, bail};
use cpal::SampleFormat;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use sherpa_onnx::LinearResampler;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use crate::cli::{Args, AudioHost};
use crate::events::AppEvent;
use crate::model::ParakeetRecognizer;

const TARGET_RATE: u32 = 16_000;

pub fn list_devices(host_kind: AudioHost) -> Result<()> {
    let host = select_host(host_kind)?;
    println!("Audio host: {}", host.id().name());
    let default_id = host.default_input_device().and_then(|d| d.id().ok());
    let default_name = host
        .default_input_device()
        .and_then(|d| d.description().ok())
        .map(|d| d.name().to_owned());
    println!(
        "Default input: {}",
        default_name.as_deref().unwrap_or("none")
    );
    let devices = host
        .input_devices()
        .context("failed to enumerate input devices")?;
    let mut count = 0;
    for device in devices {
        let name = device
            .description()
            .map(|d| d.name().to_owned())
            .unwrap_or_else(|_| "<unnamed>".into());
        let marker = if device.id().ok() == default_id {
            " (default)"
        } else {
            ""
        };
        println!("- {name}{marker}");
        if let Ok(config) = device.default_input_config() {
            println!(
                "  default format: {} Hz, {} channel(s), {:?}",
                config.sample_rate(),
                config.channels(),
                config.sample_format()
            );
        }
        count += 1;
    }
    if count == 0 {
        println!(
            "No input devices found. Ensure a microphone is connected and available via PipeWire/ALSA."
        );
    }
    Ok(())
}

pub fn microphone(engine: &ParakeetRecognizer, args: &Args) -> Result<()> {
    let (stop_tx, stop_rx) = mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = stop_tx.send(());
    })
    .context("could not install Ctrl+C handler")?;
    capture_loop(engine, args, stop_rx, None)
}

pub(crate) fn capture_loop(
    engine: &ParakeetRecognizer,
    args: &Args,
    stop_rx: mpsc::Receiver<()>,
    events: Option<mpsc::Sender<AppEvent>>,
) -> Result<()> {
    let host = select_host(args.host)?;
    let device = if let Some(filter) = &args.device {
        host.input_devices()
            .context("failed to enumerate input devices")?
            .find(|d| {
                d.description()
                    .is_ok_and(|desc| desc.name().to_lowercase().contains(&filter.to_lowercase()))
            })
            .with_context(|| format!("no input device name contains '{filter}'"))?
    } else {
        host.default_input_device()
            .context("no default microphone found; try --list-devices")?
    };
    let device_name = device
        .description()
        .map(|d| d.name().to_owned())
        .unwrap_or_else(|_| "unnamed microphone".into());
    let supported = device
        .default_input_config()
        .context("microphone has no default input configuration")?;
    let channels = supported.channels() as usize;
    let input_rate = supported.sample_rate();
    let sample_format = supported.sample_format();
    let stream_config: cpal::StreamConfig = supported.into();

    println!("Microphone: {device_name}");
    println!(
        "Capture: {input_rate} Hz, {channels} channel(s), {sample_format:?}; resampling to 16 kHz mono"
    );
    let listening_status = format!(
        "Listening; utterances end after {:.1}s of silence",
        args.silence_seconds
    );
    if let Some(events) = &events {
        let _ = events.send(AppEvent::Status(listening_status));
    } else {
        println!("Listening. Speak a sentence; Ctrl+C exits.");
    }

    // Leave several seconds of headroom while the offline model decodes a final utterance.
    let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(256);
    let dropped = Arc::new(AtomicUsize::new(0));
    let stream = build_capture_stream(
        &device,
        &stream_config,
        sample_format,
        channels,
        tx,
        dropped.clone(),
    )?;
    stream
        .play()
        .context("could not start microphone capture")?;

    let resampler = if input_rate == TARGET_RATE {
        None
    } else {
        Some(
            LinearResampler::create(input_rate as i32, TARGET_RATE as i32)
                .context("could not initialize sample-rate converter")?,
        )
    };
    let mut vad = UtteranceVad::new(
        args.vad_threshold,
        args.silence_seconds,
        TARGET_RATE as usize,
    );
    let mut capture_samples = 0_u64;
    let mut dropped_total = 0;

    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(block) => {
                capture_samples += block.len() as u64;
                let mono_16k = match &resampler {
                    Some(r) => r.resample(&block, false),
                    None => block,
                };
                for utterance in vad.push(&mono_16k) {
                    let text = print_transcription(engine, utterance, "silence");
                    if let (Some(events), Some(text)) = (&events, text) {
                        let _ = events.send(AppEvent::Transcript(text));
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                bail!("microphone capture stopped unexpectedly")
            }
        }
        dropped_total = dropped.load(Ordering::Relaxed);
    }
    drop(stream);
    if let Some(utterance) = vad.finish() {
        let text = print_transcription(engine, utterance, "stop");
        if let (Some(events), Some(text)) = (&events, text) {
            let _ = events.send(AppEvent::Transcript(text));
        }
    }
    println!(
        "\nCapture stopped. Captured {:.1} source seconds; dropped callback blocks: {dropped_total}.",
        capture_samples as f64 / input_rate as f64
    );
    Ok(())
}

fn select_host(host_kind: AudioHost) -> Result<cpal::Host> {
    let id = match host_kind {
        AudioHost::Auto => return Ok(cpal::default_host()),
        AudioHost::Pipewire => cpal::HostId::PipeWire,
        AudioHost::Alsa => cpal::HostId::Alsa,
    };
    cpal::host_from_id(id).with_context(|| format!("audio host {id} is not available"))
}

struct Utterance {
    samples: Vec<f32>,
    finalized_at: Instant,
}

struct UtteranceVad {
    threshold: f32,
    silence_samples: usize,
    sample_rate: usize,
    pre_roll: VecDeque<f32>,
    samples: Vec<f32>,
    in_speech: bool,
    silence_count: usize,
}

impl UtteranceVad {
    fn new(threshold: f32, silence_seconds: f32, sample_rate: usize) -> Self {
        Self {
            threshold,
            silence_samples: (silence_seconds * sample_rate as f32) as usize,
            sample_rate,
            pre_roll: VecDeque::with_capacity(sample_rate / 5),
            samples: Vec::new(),
            in_speech: false,
            silence_count: 0,
        }
    }

    fn push(&mut self, samples: &[f32]) -> Vec<Utterance> {
        let mut finished = Vec::new();
        for frame in samples.chunks(160) {
            // 10 ms analysis windows at 16 kHz
            let rms = (frame.iter().map(|x| x * x).sum::<f32>() / frame.len().max(1) as f32).sqrt();
            if self.in_speech {
                self.samples.extend_from_slice(frame);
                if rms < self.threshold {
                    self.silence_count += frame.len();
                } else {
                    self.silence_count = 0;
                }
                if self.silence_count >= self.silence_samples {
                    let keep = self.samples.len().saturating_sub(self.silence_count);
                    self.samples.truncate(keep);
                    if self.samples.len() > self.sample_rate / 4 {
                        finished.push(self.take());
                    } else {
                        self.reset();
                    }
                } else if self.samples.len() >= self.sample_rate * 15 {
                    finished.push(self.take());
                }
            } else if rms >= self.threshold {
                self.samples.extend(self.pre_roll.iter().copied());
                self.pre_roll.clear();
                self.samples.extend_from_slice(frame);
                self.in_speech = true;
                self.silence_count = 0;
            } else {
                self.pre_roll.extend(frame.iter().copied());
                while self.pre_roll.len() > self.sample_rate / 5 {
                    self.pre_roll.pop_front();
                }
            }
        }
        finished
    }

    fn take(&mut self) -> Utterance {
        let utterance = Utterance {
            samples: std::mem::take(&mut self.samples),
            finalized_at: Instant::now(),
        };
        self.reset();
        utterance
    }

    fn reset(&mut self) {
        self.samples.clear();
        self.in_speech = false;
        self.silence_count = 0;
    }

    fn finish(&mut self) -> Option<Utterance> {
        (self.in_speech && !self.samples.is_empty()).then(|| self.take())
    }
}

fn print_transcription(
    engine: &ParakeetRecognizer,
    utterance: Utterance,
    reason: &str,
) -> Option<String> {
    let speech_duration = utterance.samples.len() as f64 / TARGET_RATE as f64;
    let start = Instant::now();
    match engine.decode(&utterance.samples, TARGET_RATE as i32) {
        Ok(text) => {
            let inference = start.elapsed();
            let end_to_text = utterance.finalized_at.elapsed();
            if !text.trim().is_empty() {
                println!(
                    "\n[{reason}: {:.2}s audio | decode {:.3}s | end-to-text {:.3}s | RTF {:.3}]\n{text}",
                    speech_duration,
                    inference.as_secs_f64(),
                    end_to_text.as_secs_f64(),
                    inference.as_secs_f64() / speech_duration.max(0.001)
                );
                return Some(text);
            }
            None
        }
        Err(error) => {
            eprintln!("\nTranscription failed: {error:#}");
            None
        }
    }
}

fn build_capture_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: SampleFormat,
    channels: usize,
    tx: mpsc::SyncSender<Vec<f32>>,
    dropped: Arc<AtomicUsize>,
) -> Result<cpal::Stream> {
    let error_fn = |err| eprintln!("Audio stream error: {err}");
    macro_rules! build {
        ($type:ty, $convert:expr) => {{
            let tx = tx.clone();
            let dropped = dropped.clone();
            device
                .build_input_stream::<$type, _, _>(
                    config.clone(),
                    move |data: &[$type], _| {
                        let mut mono = Vec::with_capacity(data.len() / channels);
                        for frame in data.chunks(channels) {
                            let sum: f32 = frame.iter().map(|sample| ($convert)(*sample)).sum();
                            mono.push(sum / frame.len().max(1) as f32);
                        }
                        if tx.try_send(mono).is_err() {
                            dropped.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                    error_fn,
                    Some(Duration::from_millis(100)),
                )
                .context("could not open microphone input stream")
        }};
    }
    match format {
        SampleFormat::F32 => build!(f32, |x: f32| x),
        SampleFormat::I16 => build!(i16, |x: i16| x as f32 / i16::MAX as f32),
        SampleFormat::U16 => build!(u16, |x: u16| (x as f32 - 32768.0) / 32768.0),
        other => bail!("unsupported microphone sample format {other:?}; try another input device"),
    }
}

#[cfg(test)]
mod tests {
    use super::UtteranceVad;

    #[test]
    fn silence_does_not_create_an_utterance() {
        let mut vad = UtteranceVad::new(0.01, 0.5, 16_000);
        assert!(vad.push(&[0.0; 16_000]).is_empty());
        assert!(vad.finish().is_none());
    }

    #[test]
    fn voice_is_finalized_after_configured_silence() {
        let mut vad = UtteranceVad::new(0.01, 0.5, 16_000);
        assert!(vad.push(&[0.0; 16_000]).is_empty());
        assert!(vad.push(&[0.05; 16_000]).is_empty());
        let finished = vad.push(&[0.0; 8_000]);
        assert_eq!(finished.len(), 1);
        assert_eq!(finished[0].samples.len(), 19_200); // 200 ms pre-roll plus 1 s speech
        assert!(vad.push(&[0.0; 16_000]).is_empty());
    }
}

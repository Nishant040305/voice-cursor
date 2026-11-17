use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Clone, Debug, Parser)]
#[command(about = "Local Parakeet voice typing for KDE, plus CPU benchmark mode")]
pub struct Args {
    /// Directory containing encoder.int8.onnx, decoder.int8.onnx, joiner.int8.onnx and tokens.txt
    #[arg(
        long,
        default_value = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8"
        )
    )]
    pub model_dir: PathBuf,

    /// Decode a mono WAV file instead of opening the microphone
    #[arg(long)]
    pub audio: Option<PathBuf>,

    /// List input devices and exit
    #[arg(long)]
    pub list_devices: bool,

    /// Run terminal microphone capture instead of the KDE tray app
    #[arg(long)]
    pub mic_test: bool,

    /// Audio API to use; PipeWire is preferred on Fedora, ALSA can access raw hardware
    #[arg(long, value_enum, default_value_t = AudioHost::Auto)]
    pub host: AudioHost,

    /// ONNX Runtime CPU threads (benchmark 1, 2, 4 and 8)
    #[arg(long, default_value_t = 2)]
    pub threads: i32,

    /// Number of timed WAV decodes after one warm-up decode
    #[arg(long, default_value_t = 3)]
    pub repeat: usize,

    /// Microphone utterance is finalized after this many seconds of silence
    #[arg(long, default_value_t = 0.7)]
    pub silence_seconds: f32,

    /// RMS threshold for microphone voice activity detection
    #[arg(long, default_value_t = 0.012)]
    pub vad_threshold: f32,

    /// Optional input device name substring; default device is used otherwise
    #[arg(long)]
    pub device: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum AudioHost {
    Auto,
    Pipewire,
    Alsa,
}

mod audio;
mod benchmark;
mod cli;
mod events;
mod metrics;
mod model;
mod tray;

use anyhow::{Result, bail};
use clap::Parser;

use crate::audio::{list_devices, microphone};
use crate::benchmark::benchmark_wav;
use crate::cli::Args;
use crate::metrics::rss_now;
use crate::model::ParakeetRecognizer;

fn main() -> Result<()> {
    let args = Args::parse();

    if args.list_devices {
        return list_devices(args.host);
    }
    if args.threads < 1 || args.repeat < 1 {
        bail!("--threads and --repeat must be at least 1");
    }

    if args.audio.is_none() && !args.mic_test {
        return tray::run(args);
    }

    println!("Parakeet model: {}", args.model_dir.display());
    println!(
        "Requested backend: CPU; ONNX Runtime threads: {}",
        args.threads
    );
    println!("Loading model...");
    let (engine, load_time) = ParakeetRecognizer::load(&args.model_dir, args.threads)?;
    println!("Model load: {:.3} s", load_time.as_secs_f64());
    println!("RSS after load: {}", rss_now());

    if let Some(audio) = args.audio {
        benchmark_wav(&engine, &audio, args.repeat)?;
    } else {
        microphone(&engine, &args)?;
    }
    Ok(())
}

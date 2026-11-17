use anyhow::{Context, Result, bail};
use sherpa_onnx::Wave;
use std::path::Path;
use std::time::Instant;

use crate::metrics::{peak_rss, process_cpu_seconds, rss_now};
use crate::model::ParakeetRecognizer;

pub fn benchmark_wav(engine: &ParakeetRecognizer, path: &Path, repeat: usize) -> Result<()> {
    let path_string = path.to_string_lossy();
    let wave = Wave::read(&path_string)
        .with_context(|| format!("could not read WAV file {}", path.display()))?;
    let duration = wave.num_samples() as f64 / wave.sample_rate() as f64;
    if duration <= 0.0 {
        bail!("WAV file has no samples");
    }
    println!(
        "Audio: {} ({:.3} s at {} Hz)",
        path.display(),
        duration,
        wave.sample_rate()
    );

    let _ = engine.decode(wave.samples(), wave.sample_rate())?; // warm-up
    let mut times = Vec::with_capacity(repeat);
    let cpu_before = process_cpu_seconds();
    let wall_before = Instant::now();
    let mut final_text = String::new();
    for _ in 0..repeat {
        let start = Instant::now();
        final_text = engine.decode(wave.samples(), wave.sample_rate())?;
        times.push(start.elapsed());
    }
    let wall = wall_before.elapsed().as_secs_f64();
    let cpu = (process_cpu_seconds() - cpu_before).max(0.0);
    times.sort();
    let median = times[times.len() / 2].as_secs_f64();

    println!("\nTranscription: {final_text}");
    println!("\nTimed decodes: {repeat} (after 1 warm-up)");
    println!("Median decode latency: {:.3} s", median);
    println!(
        "Median RTF: {:.3} (decode time / audio duration)",
        median / duration
    );
    let cpu_cores = cpu / wall;
    let logical_cpus = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    println!(
        "Average CPU: {:.2} logical cores ({:.1}% of {} available CPUs)",
        cpu_cores,
        cpu_cores / logical_cpus as f64 * 100.0,
        logical_cpus
    );
    println!("Current RSS: {}; peak RSS: {}", rss_now(), peak_rss());
    Ok(())
}

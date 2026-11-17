use anyhow::{Context, Result, bail};
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};
use std::path::Path;
use std::time::{Duration, Instant};

pub struct ParakeetRecognizer {
    recognizer: OfflineRecognizer,
}

impl ParakeetRecognizer {
    pub fn load(model_dir: &Path, threads: i32) -> Result<(Self, Duration)> {
        for name in [
            "encoder.int8.onnx",
            "decoder.int8.onnx",
            "joiner.int8.onnx",
            "tokens.txt",
        ] {
            let path = model_dir.join(name);
            if !path.is_file() {
                bail!("required model file is missing: {}", path.display());
            }
        }

        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(
                model_dir
                    .join("encoder.int8.onnx")
                    .to_string_lossy()
                    .into_owned(),
            ),
            decoder: Some(
                model_dir
                    .join("decoder.int8.onnx")
                    .to_string_lossy()
                    .into_owned(),
            ),
            joiner: Some(
                model_dir
                    .join("joiner.int8.onnx")
                    .to_string_lossy()
                    .into_owned(),
            ),
        };
        config.model_config.tokens =
            Some(model_dir.join("tokens.txt").to_string_lossy().into_owned());
        config.model_config.model_type = Some("nemo_transducer".into());
        config.model_config.provider = Some("cpu".into());
        config.model_config.num_threads = threads;
        config.model_config.debug = false;
        config.decoding_method = Some("greedy_search".into());

        let start = Instant::now();
        let recognizer = OfflineRecognizer::create(&config)
            .context("sherpa-onnx could not create the Parakeet CPU recognizer")?;
        Ok((Self { recognizer }, start.elapsed()))
    }

    pub fn decode(&self, samples: &[f32], sample_rate: i32) -> Result<String> {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate, samples);
        self.recognizer.decode(&stream);
        Ok(stream
            .get_result()
            .context("sherpa-onnx returned no transcription result")?
            .text)
    }
}

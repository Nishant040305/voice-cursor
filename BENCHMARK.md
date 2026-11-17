# Model benchmark and microphone results

Measured on the Fedora 44 KDE/Wayland host in this workspace (Intel Core i7-13700H, 14 cores / 20 logical CPUs, 16 GB RAM, CPU-only; sherpa-onnx 2.0.1 Rust wrapper; optimized Rust build). The figures below are measurements on the bundled sherpa-onnx test WAVs, not estimates. Each run warmed the model once before the timed repeats.

## Parakeet TDT v2 INT8 thread sweep

| Model / test clip | Threads | Median decode latency | RTF | Average CPU | Peak RSS |
|---|---:|---:|---:|---:|---:|
| TDT v2 INT8 / 7.435 s English | 1 | 1.040 s | 0.140 | 0.96 cores (4.8% of 20) | 1,157 MiB |
| TDT v2 INT8 / 7.435 s English | 2 | 0.570 s | 0.077 | 3.33 cores (16.7% of 20) | 1,156 MiB |
| TDT v2 INT8 / 7.435 s English | 4 | 0.510 s | 0.069 | 8.64 cores (43.2% of 20) | 1,141 MiB |
| TDT v2 INT8 / 7.435 s English | 8 | 0.541 s | 0.073 | 17.94 cores (89.7% of 20) | 1,155 MiB |

Average CPU is process CPU time divided by wall time. Core-equivalents can exceed the requested sherpa thread count due to ONNX Runtime and native-library worker threads; percentages are normalized to the host's 20 logical CPUs. CPU activity is bursty during each offline decode, not continuous while idle.

The v2 test WAV is 7.435 seconds at 16 kHz; the example transcript matched its bundled reference sentence.

Model loading took 1.7–2.0 seconds. Resident memory after model load was about 0.95–1.00 GiB; observed peak RSS across the runs was 1,043–1,157 MiB, comfortably below the 2.5 GB target.

## Microphone smoke test and current validation status

During the earlier live smoke test, CPAL's default PipeWire input failed with `no target node available`; `pactl list short sources` showed only the speaker monitor. The onboard `HDA Intel PCH / CX11880 Analog` advertised an ALSA capture config (48 kHz, stereo, float32) and could be opened directly, but raw ALSA capture was inconsistent: one run produced a transcript from 6.06 seconds of audio at 2 threads (0.482 s decode, RTF 0.080, 0 callback queue blocks); follow-up runs captured audio but did not consistently trigger VAD, and one showed repeated ALSA overrun warnings. A later, non-recording `--list-devices` check now sees PipeWire `default_input`, Built-in Audio, and OnePlus Nord Buds 2 input devices. That confirms device enumeration only; a fresh live capture and tray-to-focused-field test have not been run.

For end-to-end validation, choose an input in KDE **System Settings → Audio → Devices**, then run `--mic-test` or start dictation from the tray. This project does not change the user's system audio profile automatically. A controlled natural-pause end-to-text latency and focused-field paste result remain pending.

## Model decision and application validation

The project uses **TDT v2 INT8 with 2 inference threads**: two threads decoded the test clip in 0.57 s at substantially lower CPU use than four threads (0.51 s). Eight threads used nearly all logical CPUs and was slower. Increase to four only if lower utterance latency is worth the additional CPU use. The model works reliably on the test audio; repeatable live-microphone reliability remains unconfirmed until the KDE input source is enabled.

The model is an offline recognizer. The app uses silence/VAD segmentation, then decodes a completed utterance. It is not model-native token-by-token streaming and does not emit revisable partial hypotheses. The KDE tray UI, XDG GlobalShortcuts portal integration, clipboard paste path, and optional `ydotool` typing are implemented; end-to-end audio and focused-field typing still need validation with an active microphone and a running `ydotoold` service.

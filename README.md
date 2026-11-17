# Voice Typing

Local English dictation for Fedora KDE on Wayland. Speech recognition runs on the CPU with sherpa-onnx and Parakeet TDT v2 INT8; audio and transcripts are not sent to a network service. The app lives in the KDE system tray and supports a global shortcut, microphone start/stop, utterance transcription, and automatic paste into the focused app.

Parakeet TDT v2 is an offline model, not a token-by-token streaming model. Voice activity detection waits for a pause (0.7 seconds by default), then decodes that completed utterance.

## Install dependencies

```sh
sudo dnf install gcc gcc-c++ make cmake alsa-lib-devel pipewire-devel \
  pkgconf-pkg-config curl bzip2 wl-clipboard ydotool
sudo systemctl enable --now ydotool.service
```

`ydotoold` needs access to `/dev/uinput`; Fedora's `ydotool.service` sets up the daemon. With automatic typing enabled, the app writes each recognized phrase to the Wayland clipboard and sends Ctrl+V through `ydotool`, leaving the clipboard with the most recent phrase. Automatic typing can be turned off from the tray menu; **Copy last transcript** remains available.

On Fedora, the system `ydotool.service` may create a root-owned socket at `/tmp/.ydotool_socket`, while the user-side `ydotool` command looks in `$XDG_RUNTIME_DIR/.ydotool_socket`. If typing reports that it cannot connect to the socket, configure the service to create the socket in your user runtime directory and give it to your user. Find your numeric IDs with `id -u` and `id -g`, then run `sudo systemctl edit ydotool.service` and add the following, replacing `<UID>` and `<GID>` with those numbers:

```ini
[Service]
ExecStart=
ExecStart=/usr/bin/ydotoold --socket-path=/run/user/<UID>/.ydotool_socket --socket-perm=0600 --socket-own=<UID>:<GID>
```

Apply it with `sudo systemctl daemon-reload && sudo systemctl restart ydotool.service`. The socket should then be owned by your user at `$XDG_RUNTIME_DIR/.ydotool_socket`.

## Download and build

```sh
./scripts/download-model.sh v2
cargo build --release
```

The sole configured model is `models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8`. Model files are ignored by Git and may be removed independently of the code. The first Cargo build downloads the sherpa-onnx CPU runtime; inference itself is local. Two ONNX threads are the default.

## Run the tray app

```sh
./target/release/voice-typing
```

The microphone icon appears in the KDE system tray immediately; the speech model stays unloaded while idle. Left-click toggles dictation; right-click opens the menu with start/stop, automatic typing, copy-last, shortcut configuration, and quit. Starting dictation loads the model (about 1.8 seconds on the benchmarked machine); stopping dictation releases it and its memory. The app requests a shortcut through the XDG GlobalShortcuts portal on startup, preferring **Ctrl+Alt+Space**. Plasma may ask you to confirm or choose the binding. Use **Configure global shortcut…** in the menu to change it later. If the portal is unavailable, tray start/stop still works.

This machine is configured to start the tray app automatically at KDE login using `~/.config/autostart/voice-typing.desktop`. Disable it in **System Settings → Autostart** or remove that file if you no longer want it to launch automatically.

For model/device selection and audio tuning, use the command-line options:

```sh
./target/release/voice-typing --model-dir models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8 \
  --host pipewire --device "Microphone" --threads 2 \
  --silence-seconds 0.7 --vad-threshold 0.012
```

Select `--host alsa` when direct ALSA capture is needed. Enable an input-capable profile and microphone in **System Settings → Audio → Devices** if no default microphone is available.

## Microphone and benchmark modes

List devices, or run a terminal capture test without tray or automatic paste:

```sh
./target/release/voice-typing --list-devices
./target/release/voice-typing --mic-test
./target/release/voice-typing --host alsa --mic-test --device "HDA Intel PCH"
```

Benchmark a WAV file (warm-up decode is excluded from the timed repeats):

```sh
./target/release/voice-typing \
  --audio models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/test_wavs/0.wav \
  --threads 2 --repeat 5
```

See [`BENCHMARK.md`](BENCHMARK.md) for CPU, memory, and microphone test results. The current KDE session had no active built-in microphone source during the last live test, so enable a duplex/input profile before evaluating end-to-end capture reliability.

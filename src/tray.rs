use anyhow::{Context, Result, bail};
use ashpd::desktop::CreateSessionOptions;
use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
use futures_util::StreamExt;
use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{MenuItem, ToolTip};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::audio::capture_loop;
use crate::cli::Args;
use crate::events::AppEvent;
use crate::metrics::rss_now;
use crate::model::ParakeetRecognizer;

struct TrayApp {
    events: mpsc::Sender<AppEvent>,
    portal_commands: tokio::sync::mpsc::UnboundedSender<PortalCommand>,
    status: String,
    recording: bool,
    auto_paste: bool,
    last_transcript: Option<String>,
}

impl ksni::Tray for TrayApp {
    fn id(&self) -> String {
        "voice-typing".into()
    }

    fn title(&self) -> String {
        "Voice Typing".into()
    }

    fn icon_name(&self) -> String {
        if self.recording {
            "audio-input-microphone-high".into()
        } else {
            "audio-input-microphone".into()
        }
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            icon_name: self.icon_name(),
            title: "Voice Typing".into(),
            description: self.status.clone(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.events.send(AppEvent::Toggle);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let events = self.events.clone();
        let recording_label = if self.recording {
            "Stop dictation"
        } else {
            "Start dictation"
        };
        let mut items = vec![
            StandardItem {
                label: recording_label.into(),
                icon_name: if self.recording {
                    "media-playback-stop".into()
                } else {
                    "media-record".into()
                },
                activate: Box::new(move |_| {
                    let _ = events.send(AppEvent::Toggle);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: format!("Status: {}", self.status),
                enabled: false,
                ..Default::default()
            }
            .into(),
        ];

        let events = self.events.clone();
        items.push(
            CheckmarkItem {
                label: "Type transcripts automatically".into(),
                checked: self.auto_paste,
                activate: Box::new(move |_| {
                    let _ = events.send(AppEvent::ToggleAutoPaste);
                }),
                ..Default::default()
            }
            .into(),
        );

        let events = self.events.clone();
        items.push(
            StandardItem {
                label: "Copy last transcript".into(),
                enabled: self.last_transcript.is_some(),
                activate: Box::new(move |_| {
                    let _ = events.send(AppEvent::CopyLast);
                }),
                ..Default::default()
            }
            .into(),
        );

        let events = self.events.clone();
        items.push(
            StandardItem {
                label: "Configure global shortcut…".into(),
                activate: Box::new(move |_| {
                    let _ = events.send(AppEvent::ConfigureShortcut);
                }),
                ..Default::default()
            }
            .into(),
        );

        let events = self.events.clone();
        items.push(
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(move |_| {
                    let _ = events.send(AppEvent::Quit);
                }),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}

pub fn run(args: Args) -> Result<()> {
    let (events_tx, events_rx) = mpsc::channel();
    let portal_commands = start_shortcut_portal(events_tx.clone());
    let ctrlc_events = events_tx.clone();
    ctrlc::set_handler(move || {
        let _ = ctrlc_events.send(AppEvent::Quit);
    })
    .context("could not install Ctrl+C handler")?;

    let tray = TrayApp {
        events: events_tx.clone(),
        portal_commands,
        status: "Ready — model loads when dictation starts".into(),
        recording: false,
        auto_paste: true,
        last_transcript: None,
    };
    let handle = tray
        .spawn()
        .context("could not start KDE system tray item")?;
    let mut stop_capture: Option<mpsc::Sender<()>> = None;

    loop {
        match events_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(AppEvent::Toggle) if !is_recording(&handle) => {
                let (stop_tx, stop_rx) = mpsc::channel();
                let worker_events = events_tx.clone();
                let worker_args = args.clone();
                thread::spawn(move || {
                    println!("Loading Parakeet model for dictation...");
                    let (engine, load_time) =
                        match ParakeetRecognizer::load(&worker_args.model_dir, worker_args.threads)
                        {
                            Ok(loaded) => loaded,
                            Err(error) => {
                                let _ = worker_events
                                    .send(AppEvent::Status(format!("Model error: {error:#}")));
                                let _ = worker_events.send(AppEvent::WorkerStopped);
                                return;
                            }
                        };
                    println!(
                        "Model load: {:.3} s; RSS: {}",
                        load_time.as_secs_f64(),
                        rss_now()
                    );

                    // If the user stopped while the model was loading, don't open the mic.
                    if !matches!(stop_rx.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                        let _ = worker_events.send(AppEvent::WorkerStopped);
                        return;
                    }

                    if let Err(error) =
                        capture_loop(&engine, &worker_args, stop_rx, Some(worker_events.clone()))
                    {
                        let _ = worker_events
                            .send(AppEvent::Status(format!("Microphone error: {error:#}")));
                    }
                    let _ = worker_events.send(AppEvent::WorkerStopped);
                });
                stop_capture = Some(stop_tx);
                handle.update(|tray| {
                    tray.recording = true;
                    tray.status = "Loading speech model…".into();
                });
            }
            Ok(AppEvent::Toggle) => {
                if let Some(stop) = &stop_capture {
                    let _ = stop.send(());
                    handle.update(|tray| tray.status = "Stopping dictation…".into());
                }
            }
            Ok(AppEvent::ToggleAutoPaste) => {
                handle.update(|tray| {
                    tray.auto_paste = !tray.auto_paste;
                    tray.status = if tray.auto_paste {
                        "Automatic typing enabled".into()
                    } else {
                        "Automatic typing disabled; use Copy last transcript".into()
                    };
                });
            }
            Ok(AppEvent::CopyLast) => {
                let text = handle.update(|tray| tray.last_transcript.clone()).flatten();
                if let Some(text) = text {
                    let status = match copy_to_clipboard(&text) {
                        Ok(()) => "Transcript copied to clipboard".into(),
                        Err(error) => format!("Clipboard error: {error:#}"),
                    };
                    handle.update(|tray| tray.status = status);
                }
            }
            Ok(AppEvent::ConfigureShortcut) => {
                let sent = handle
                    .update(|tray| tray.portal_commands.send(PortalCommand::Configure).is_ok());
                if sent == Some(false) {
                    handle.update(|tray| {
                        tray.status = "Global-shortcut portal is unavailable".into()
                    });
                }
            }
            Ok(AppEvent::Status(status)) => {
                handle.update(|tray| tray.status = status);
            }
            Ok(AppEvent::Transcript(text)) => {
                let auto_paste = handle
                    .update(|tray| {
                        tray.last_transcript = Some(text.clone());
                        tray.auto_paste
                    })
                    .unwrap_or(false);
                if auto_paste {
                    let text_to_type = format!("{} ", text.trim_end());
                    let result = copy_to_clipboard(&text_to_type).and_then(|()| paste_clipboard());
                    handle.update(|tray| {
                        tray.status = match result {
                            Ok(()) => "Transcript typed; still listening".into(),
                            Err(error) => format!("Paste error: {error:#}"),
                        };
                    });
                } else {
                    handle.update(|tray| tray.status = "Transcript ready to copy".into());
                }
            }
            Ok(AppEvent::WorkerStopped) => {
                stop_capture = None;
                handle.update(|tray| {
                    tray.recording = false;
                    if tray.status.starts_with("Loading speech model")
                        || tray.status.starts_with("Starting microphone")
                        || tray.status.starts_with("Listening")
                        || tray.status.starts_with("Stopping")
                    {
                        tray.status = "Ready".into();
                    }
                });
            }
            Ok(AppEvent::Quit) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if let Some(stop) = stop_capture {
        let _ = stop.send(());
    }
    handle.shutdown().wait();
    Ok(())
}

fn is_recording(handle: &ksni::blocking::Handle<TrayApp>) -> bool {
    handle.update(|tray| tray.recording).unwrap_or(false)
}

fn start_shortcut_portal(
    events: mpsc::Sender<AppEvent>,
) -> tokio::sync::mpsc::UnboundedSender<PortalCommand> {
    let (commands_tx, mut commands_rx) = tokio::sync::mpsc::unbounded_channel();
    thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                let _ = events.send(AppEvent::Status(format!(
                    "Could not start shortcut service: {error}"
                )));
                return;
            }
        };
        runtime.block_on(async move {
            let setup = async {
                let portal = GlobalShortcuts::new().await?;
                let session = portal
                    .create_session(CreateSessionOptions::default())
                    .await?;
                let mut activated = portal.receive_activated().await?;
                let shortcut = NewShortcut::new("toggle-recording", "Toggle voice dictation")
                    .preferred_trigger("CTRL+ALT+SPACE");
                let request = portal
                    .bind_shortcuts(&session, &[shortcut], None, BindShortcutsOptions::default())
                    .await?;
                let binding = request.response()?;
                let configured = binding
                    .shortcuts()
                    .first()
                    .map(|shortcut| shortcut.trigger_description().to_owned())
                    .filter(|trigger| !trigger.is_empty())
                    .unwrap_or_else(|| "use tray menu".into());
                let _ = events.send(AppEvent::Status(format!(
                    "Ready; toggle shortcut: {configured}"
                )));

                loop {
                    tokio::select! {
                        signal = activated.next() => {
                            match signal {
                                Some(signal) if signal.shortcut_id() == "toggle-recording" => {
                                    let _ = events.send(AppEvent::Toggle);
                                }
                                Some(_) => {}
                                None => break,
                            }
                        }
                        command = commands_rx.recv() => {
                            match command {
                                Some(PortalCommand::Configure) => {
                                    if let Err(error) = portal
                                        .configure_shortcuts(&session, None, Default::default())
                                        .await
                                    {
                                        let _ = events.send(AppEvent::Status(format!(
                                            "Shortcut configuration failed: {error}"
                                        )));
                                    }
                                }
                                None => break,
                            }
                        }
                    }
                }
                Ok::<(), ashpd::Error>(())
            };
            if let Err(error) = setup.await {
                let _ = events.send(AppEvent::Status(format!(
                    "Global shortcut unavailable: {error}"
                )));
            }
        });
    });
    commands_tx
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .args(["--type", "text/plain;charset=utf-8"])
        .stdin(Stdio::piped())
        .spawn()
        .context("wl-copy is required to put dictated text on the Wayland clipboard")?;
    child
        .stdin
        .take()
        .context("could not open wl-copy input")?
        .write_all(text.as_bytes())
        .context("could not send transcript to wl-copy")?;
    let status = child.wait().context("could not wait for wl-copy")?;
    if !status.success() {
        bail!("wl-copy exited with {status}");
    }
    Ok(())
}

fn paste_clipboard() -> Result<()> {
    let status = Command::new("ydotool")
        .args(["key", "29:1", "47:1", "47:0", "29:0"])
        .status()
        .context("ydotool is required to paste into the focused app on Wayland")?;
    if !status.success() {
        bail!("ydotool exited with {status}; check that ydotoold is running and has uinput access");
    }
    Ok(())
}

enum PortalCommand {
    Configure,
}

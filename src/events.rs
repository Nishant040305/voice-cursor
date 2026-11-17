#[derive(Debug)]
pub enum AppEvent {
    Toggle,
    ToggleAutoPaste,
    CopyLast,
    ConfigureShortcut,
    Quit,
    Status(String),
    Transcript(String),
    WorkerStopped,
}

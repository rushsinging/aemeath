use crossterm::event::{KeyEvent, MouseEvent};

/// Unified message type for the TEA event loop.
/// All events (terminal, UI, async) flow through this single enum.
#[derive(Debug)]
pub enum Msg {
    // --- Terminal events ---
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    #[allow(dead_code)]
    Resize(u16, u16),
    #[allow(dead_code)]
    Tick,

    // --- Async UI events (from background LLM processing) ---
    Ui(super::UiEvent),
}

/// Commands describe side effects that the runtime should execute.
/// update() returns these instead of doing IO directly.
pub(crate) enum Cmd {
    /// No side effect.
    None,
    /// Quit the application.
    Quit,
    /// Spawn background LLM processing with the given context.
    SpawnProcessing(super::processing::SpawnContext),
    /// Send a batch of UI events (used for async clipboard/image operations).
    #[allow(dead_code)]
    SendEvents(Vec<super::UiEvent>),
    /// Queue a user input for processing after current work finishes.
    #[allow(dead_code)]
    QueueInput(String),
    /// Save session with the given messages (async operation).
    SaveSession(Vec<aemeath_core::message::Message>),
}

impl Cmd {
    /// Convenience: batch multiple commands into one.
    /// Returns the first non-None command, or Cmd::None.
    #[allow(dead_code)]
    pub fn batch(cmds: Vec<Cmd>) -> Cmd {
        for cmd in cmds {
            match cmd {
                Cmd::None => continue,
                other => return other,
            }
        }
        Cmd::None
    }
}

use crossterm::event::{KeyEvent, MouseEvent};

/// Unified message type for the TEA event loop.
/// All events (terminal, UI, async) flow through this single enum.
#[derive(Debug)]
pub enum Msg {
    // --- Terminal events ---
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    Resize { width: u16, height: u16 },
    SpinnerTick,

    // --- Async UI events (from background LLM processing) ---
    Ui(super::event::UiEvent),
}

/// Commands describe side effects that the runtime should execute.
/// update() returns these instead of doing IO directly.
///
/// run_loop.rs 负责执行所有异步副作用（通过 AgentClient），不再委派给 CmdExecutor。
pub(crate) enum Cmd {
    /// No side effect.
    None,
    /// Quit the application.
    Quit,
    /// Spawn background LLM processing with the given context.
    SpawnProcessing(crate::tui::session::processing::SpawnContext),
    /// Save session (run_loop handles via AgentClient).
    SaveCurrentSession,
    /// Run a hook notification (run_loop handles via AgentClient).
    RunHookNotification { message: String, kind: String },
    /// Read clipboard image (run_loop handles via AgentClient).
    ReadClipboardImage,
    /// Process an image file path (run_loop handles via AgentClient).
    ProcessImageFile(String),
    /// 记录当前 turn（由 CLI 边界转发给 runtime bootstrap）。
    SetCurrentTurn(usize),
    /// 异步获取 session reminders 并推送 recap 行。
    FetchReminderRecap,
}

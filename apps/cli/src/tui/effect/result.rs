#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectResult {
    Noop,
    RenderRequested,
    AgentChatSpawned { chat_id: String },
    SessionSaved,
    TaskStatusFetched,
    ClipboardCopied,
    HookFinished { name: String, success: bool },
    TimerStarted { id: String },
    TimerStopped { id: String },
    Failed { message: String },
}

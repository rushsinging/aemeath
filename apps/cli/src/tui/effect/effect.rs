pub struct SpawnAgentChatEffect {
    pub chat_id: String,
    pub prompt: String,
    pub context: Option<crate::tui::effect::session::processing::SpawnContext>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    None,
    QuitApplication,
    RequestRender,
    SpawnAgentChat {
        chat_id: String,
        prompt: String,
    },
    SendChatInputEvent {
        event: sdk::ChatInputEvent,
    },
    CancelCurrentRun,
    /// 保存当前会话。`notify=true`（/save 手动触发）时经 UiEvent 回灌
    /// `[session saved: id]` / 失败反馈；`false`（MessagesSync 后台自动保存）静默。
    SaveSession {
        notify: bool,
    },
    FetchReminderRecap,
    /// 拉取 reminder 列表（/memory 命令），结果经 UiEvent::MemoryList 回灌。
    FetchMemoryList,
    CopyToClipboard {
        text: String,
    },
    ReadClipboardImage,
    ProcessImageFile {
        path: String,
    },
    /// 查询最近的 reflection 历史；只向 runtime 推送查询事件，不触发 LLM。
    QueryReflectionHistory {
        limit: usize,
    },
    RunHook {
        name: String,
        message: String,
    },
    SetCurrentTurn {
        turn: usize,
    },
    StartTimer {
        id: String,
    },
    StopTimer {
        id: String,
    },
    /// 执行自动更新（`/update` 命令触发）。
    RunSelfUpdate,
    /// 重置 per-conversation runtime 状态（清空消息/输出/任务/UI 状态）。
    /// 由 SessionReset 事件触发（runtime idle gate 处理 Reset 后回灌）。
    ResetRuntimeState,
    /// 用系统默认程序打开 URL（Ctrl+Click markdown link）。
    OpenUrl {
        url: String,
    },
}

impl Effect {
    pub fn is_noop(&self) -> bool {
        matches!(self, Effect::None)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectResult {
    Noop,
    SessionSaved,
    Failed { message: String },
}

impl EffectResult {
    pub fn session_saved() -> Self {
        Self::SessionSaved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_request_render_is_pure_value() {
        let effect = Effect::RequestRender;
        assert!(!effect.is_noop());
    }

    #[test]
    fn test_spawn_agent_chat_carries_chat_id() {
        let effect = Effect::SpawnAgentChat {
            chat_id: "chat-1".to_string(),
            prompt: "hello".to_string(),
        };
        assert!(
            matches!(effect, Effect::SpawnAgentChat { ref chat_id, .. } if chat_id == "chat-1")
        );
    }

    #[test]
    fn test_effect_none_is_distinct_from_render() {
        assert!(!Effect::RequestRender.is_noop());
    }
}

pub struct SpawnAgentChatEffect {
    pub context: Option<crate::tui::effect::session::processing::SpawnContext>,
}

use crate::tui::model::conversation::interaction::{
    UiInteractionCancelReason, UiInteractionReply, UiInteractionRequestId,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    QuitApplication,
    RequestRender,
    SendChatInputEvent {
        event: sdk::ChatInputEvent,
    },
    LoadDisplayHistoryWindow {
        request: sdk::DisplayHistoryWindowRequest,
    },
    CancelRunStep {
        run_id: sdk::RunId,
        step_id: sdk::RunStepId,
    },
    ReplyInteraction {
        request_id: UiInteractionRequestId,
        reply: UiInteractionReply,
    },
    CancelInteraction {
        request_id: UiInteractionRequestId,
        reason: UiInteractionCancelReason,
    },
    ResolveWorkspaceMetadata {
        root: String,
        revision: u64,
    },
    /// 保存当前会话。`notify=true`（/save 手动触发）时经 UiEvent 回灌
    /// `[session saved: id]` / 失败反馈；`false`（MessagesSync 后台自动保存）静默。
    SaveSession {
        notify: bool,
    },
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
    /// 执行自动更新（`/update` 命令触发）。
    RunSelfUpdate,
    /// 重置 per-conversation runtime 状态（清空消息/输出/任务/UI 状态）。
    /// 由 SessionReset 事件触发（runtime idle gate 处理 Reset 后回灌）。
    ResetRuntimeState,
    /// 用系统默认程序打开 URL（Cmd+Click markdown link）。
    OpenUrl {
        url: String,
    },
}

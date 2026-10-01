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
    /// 打开 Provider Connect 向导（/connect）。需独占终端，由 run_loop 层
    /// 处理（挂起主 TUI → 全屏表单 → 恢复），不经 async executor。
    OpenConnectWizard,
    SendChatInputEvent {
        event: sdk::ChatInputEvent,
    },
    LoadDisplayHistoryWindow {
        request: sdk::DisplayHistoryWindowRequest,
    },
    /// 取消当前执行单元（无 identity，Runtime 控制面裁决唯一当前 Main Run：
    /// 有 active step 走 CancelStep 协议；Manual Reflection 按 intent 取消执行体；
    /// Conversation 的 step 间隙返回 NoActiveStep）。这是 TUI 唯一取消入口——
    /// TUI 不持有 run/step identity 取消面（可寻址控制面仅供 Server/Coordinator
    /// 管理端使用）。
    CancelCurrentRun,
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
    CopyToClipboard {
        text: String,
    },
    ReadClipboardImage,
    /// 加载粘贴进来的本地图片；`path` 已由 SDK 解码，`fallback_text` 保留原始粘贴文本，
    /// 供文件不可用时回填输入区。
    ProcessImageFile {
        path: String,
        fallback_text: String,
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
    /// 回合完成时向终端发送 OSC 777 桌面通知（cmux 等终端零配置提醒）。
    SendTerminalNotification {
        title: String,
        body: String,
    },
}

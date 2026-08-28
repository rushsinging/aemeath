//! Conversation intent：每个 variant 是独立 struct，enum 仅做传输容器。
//!
//! struct 的 `impl ConversationUpdate` 逻辑在 `intent_impls.rs`。

use super::block::AskUserSlot;
use super::ids::{ChatId, ChatRunId, ToolCallId};
use super::interaction::{InteractionCommandFailure, InteractionRequest, UiInteractionRequestId};
use super::status_notice::StatusNotice;
use super::tool_call::ToolCallStatus;
use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiResumedSessionStep};
use std::time::Instant;

// ════════════════════════════════════════════════════════════════════
//  Conversation intent structs（原 ConversationIntent enum 的 27 个 variant）
// ════════════════════════════════════════════════════════════════════

/// 测试脚手架：直接建立带 active chat 的会话状态。生产路径经
/// `ensure_runtime_turn` 懒创建，不再显式开启 chat。
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(test)]
pub struct StartChat {
    pub submission: String,
}

/// 恢复历史会话消息，不触发 spinner 副作用。
///
/// 与 `StartChat` 的区别：resume 场景下 chat 已结束，不需要 spinner。
/// 传入完整消息列表，内部逐条 apply 已有 intent 灌入 ConversationModel。
#[derive(Clone, Debug, PartialEq)]
pub struct ResumeConversation {
    pub steps: Vec<TuiResumedSessionStep>,
}

/// 仅追加一条用户消息回显块，不创建新的 chat/run。
///
/// 用于 ask_user 应答、队列输入冲刷等「在已激活的对话run内回显用户输入」的场景。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendUserMessage {
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssistantText {
    pub chat_id: ChatId,
    pub run_id: ChatRunId,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThinkingText {
    pub chat_id: ChatId,
    pub run_id: ChatRunId,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompleteBlock {
    pub chat_id: ChatId,
    pub run_id: ChatRunId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallStart {
    pub chat_id: ChatId,
    pub run_id: ChatRunId,
    pub id: ToolCallId,
    pub provider_id: Option<String>,
    pub name: String,
    pub index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCallUpdate {
    pub chat_id: ChatId,
    pub run_id: ChatRunId,
    pub id: ToolCallId,
    pub provider_id: Option<String>,
    pub name: String,
    pub index: usize,
    pub arguments: Option<String>,
    pub status: ToolCallStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolResult {
    pub chat_id: ChatId,
    pub run_id: ChatRunId,
    pub id: ToolCallId,
    pub provider_id: String,
    pub tool_name: String,
    pub output: String,
    pub content: serde_json::Value,
    pub is_error: bool,
    pub image_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalNotice {
    pub cause: super::terminal::TerminalCause,
    pub duration: Option<std::time::Duration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresentCancelledStep {
    pub confirmed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendHookNotice {
    pub title: String,
    pub text: String,
    pub kind: crate::tui::adapter::runtime_view::TuiHookNoticeKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendSystemMessage {
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendError {
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueSubmission {
    pub input_id: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClearQueuedSubmissionById {
    pub input_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClearAllQueuedSubmissions;

/// 测试脚手架：批量灌入 agent activity 的唯一 apply 入口；生产经
/// `RecordSubRunActivity` 逐条驱动。
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(test)]
pub struct RecordAgentActivities {
    pub chat_id: ChatId,
    pub run_id: ChatRunId,
    pub tool_id: ToolCallId,
    pub activities: Vec<crate::tui::model::conversation::agent_activity::AgentActivityLine>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecordSubRunActivity {
    pub agent_id: String,
    pub sub_run_id: String,
    pub parent_run_id: String,
    pub spawned_by_tool_call_id: ToolCallId,
    pub sequence: u64,
    pub sequence_index: u32,
    pub kind: crate::tui::adapter::tui_runtime_event::TuiSubRunActivityKind,
}

/// 工具 stdout 流式输出（如 Bash 长输出命令的逐行 stdout）。
/// 由 TUI ACL 消费 `ToolOutputDelta` 后触发，直接写入 `ToolCall.streaming_preview`，
/// 供 TUI 实时 tail 显示。与结构化 Sub Run activity 语义独立。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordToolStreamingOutput {
    pub chat_id: ChatId,
    pub run_id: ChatRunId,
    pub tool_id: ToolCallId,
    pub text: String,
}

/// 更新 Agent 工具的元数据（issue #499）。
/// 由 compatibility ACL 翻译的 Started activity 触发，携带 sub-agent resolve 后的
/// role/model，用于 header 渲染。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateAgentMeta {
    pub chat_id: ChatId,
    pub run_id: ChatRunId,
    pub tool_id: ToolCallId,
    pub role: Option<String>,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShowAskUserBatch {
    pub request_id: super::interaction::UiInteractionRequestId,
    pub slots: Vec<AskUserSlot>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnswerCurrentAskUser {
    pub answer: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetAskUserCursor {
    pub cursor: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToggleAskUserSelected {
    pub index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetAskUserChatInput {
    pub active: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendAskUserChatChar {
    pub ch: char,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteAskUserChatChar;

/// 移动 Type something 输入框光标。
/// delta 为 char 数偏移：负数向左、正数向右。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveAskUserChatCursor {
    pub delta: isize,
}

/// 将 Type something 输入框光标移到行首（to_end=false）或行尾（to_end=true）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MoveAskUserChatCursorEnd {
    pub to_end: bool,
}

/// 删除 Type something 输入框光标前一个单词（Ctrl+W）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteAskUserChatWord;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigateAskUserTo {
    pub index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetAskUserConfirmCursor {
    pub cursor: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfirmAskUserBatch;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DismissAskUserBatch;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShowInteraction {
    pub request: InteractionRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InteractionReplyAccepted {
    pub request_id: UiInteractionRequestId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InteractionCancelAccepted {
    pub request_id: UiInteractionRequestId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InteractionReplyRejected {
    pub request_id: UiInteractionRequestId,
    pub failure: InteractionCommandFailure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InteractionCancelRejected {
    pub request_id: UiInteractionRequestId,
    pub failure: InteractionCommandFailure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompleteChat {
    pub chat_id: ChatId,
    pub run_id: ChatRunId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObserveActivityChange {
    pub kind: crate::tui::adapter::tui_runtime_event::TuiActivityChangeKind,
    pub activity: crate::tui::adapter::tui_runtime_event::TuiActivityObservation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceActivitySnapshot {
    pub snapshot: crate::tui::adapter::tui_runtime_event::TuiActivitySnapshot,
}

// ════════════════════════════════════════════════════════════════════
//  Runtime intent structs
// ════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq)]
pub struct RecordUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub last_input_tokens: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecordLiveTps {
    pub tps: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReplaceRuntimeStatus(pub crate::tui::adapter::runtime_status::TuiRuntimeStatus);

#[derive(Clone, Debug, PartialEq)]
pub struct ReplaceTaskState(pub crate::tui::adapter::runtime_view::TuiTaskState);

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateTaskLines(pub Vec<String>);

#[derive(Clone, Debug, PartialEq)]
pub struct SetStatusNotice(pub StatusNotice);

#[derive(Clone, Debug, PartialEq)]
pub struct SetTransientStatusNotice {
    pub notice: StatusNotice,
    pub expires_at: Instant,
}

#[derive(Clone, Debug)]
pub struct SyncQueuedSubmissions {
    pub queued: Vec<TuiChatMessage>,
}

impl PartialEq for SyncQueuedSubmissions {
    fn eq(&self, other: &Self) -> bool {
        self.queued.len() == other.queued.len()
            && self.queued.iter().zip(&other.queued).all(|(left, right)| {
                left.input_id == right.input_id && left.text_content() == right.text_content()
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClearCompactRuntime;

// ════════════════════════════════════════════════════════════════════
//  传输容器 enum
// ════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq)]
pub enum ConversationIntent {
    #[cfg(test)]
    StartChat(StartChat),
    #[cfg(test)]
    RecordAgentActivities(RecordAgentActivities),
    // ── 原 conversation variants ──
    ResumeConversation(ResumeConversation),
    AppendUserMessage(AppendUserMessage),
    AssistantText(AssistantText),
    ThinkingText(ThinkingText),
    CompleteBlock(CompleteBlock),
    ToolCallStart(ToolCallStart),
    ToolCallUpdate(ToolCallUpdate),
    ToolResult(ToolResult),
    TerminalNotice(TerminalNotice),
    PresentCancelledStep(PresentCancelledStep),
    AppendHookNotice(AppendHookNotice),
    AppendSystemMessage(AppendSystemMessage),
    AppendError(AppendError),
    QueueSubmission(QueueSubmission),
    ClearQueuedSubmissionById(ClearQueuedSubmissionById),
    ClearAllQueuedSubmissions(ClearAllQueuedSubmissions),
    RecordSubRunActivity(RecordSubRunActivity),
    RecordToolStreamingOutput(RecordToolStreamingOutput),
    UpdateAgentMeta(UpdateAgentMeta),
    ShowAskUserBatch(ShowAskUserBatch),
    AnswerCurrentAskUser(AnswerCurrentAskUser),
    SetAskUserCursor(SetAskUserCursor),
    ToggleAskUserSelected(ToggleAskUserSelected),
    SetAskUserChatInput(SetAskUserChatInput),
    AppendAskUserChatChar(AppendAskUserChatChar),
    DeleteAskUserChatChar(DeleteAskUserChatChar),
    MoveAskUserChatCursor(MoveAskUserChatCursor),
    MoveAskUserChatCursorEnd(MoveAskUserChatCursorEnd),
    DeleteAskUserChatWord(DeleteAskUserChatWord),
    NavigateAskUserTo(NavigateAskUserTo),
    SetAskUserConfirmCursor(SetAskUserConfirmCursor),
    ConfirmAskUserBatch(ConfirmAskUserBatch),
    DismissAskUserBatch(DismissAskUserBatch),
    ShowInteraction(ShowInteraction),
    InteractionReplyAccepted(InteractionReplyAccepted),
    InteractionCancelAccepted(InteractionCancelAccepted),
    InteractionReplyRejected(InteractionReplyRejected),
    InteractionCancelRejected(InteractionCancelRejected),
    ObserveActivityChange(ObserveActivityChange),
    ReplaceActivitySnapshot(ReplaceActivitySnapshot),
    CompleteChat(CompleteChat),
    // ── 原 runtime variants ──
    RecordUsage(RecordUsage),
    RecordLiveTps(RecordLiveTps),
    ReplaceRuntimeStatus(ReplaceRuntimeStatus),
    ReplaceTaskState(ReplaceTaskState),
    UpdateTaskLines(UpdateTaskLines),
    SetStatusNotice(SetStatusNotice),
    SetTransientStatusNotice(SetTransientStatusNotice),
    SyncQueuedSubmissions(SyncQueuedSubmissions),
    ClearCompactRuntime(ClearCompactRuntime),
}

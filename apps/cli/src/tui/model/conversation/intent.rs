//! Conversation intent：每个 variant 是独立 struct，enum 仅做传输容器。
//!
//! struct 的 `impl ConversationUpdate` 逻辑在 `intent_impls.rs`。

use super::block::{AskUserSlot, HookNoticeContent};
use super::ids::{ChatId, ChatTurnId, ToolCallId};
use super::status_notice::StatusNotice;
use super::tool_call::ToolCallStatus;
use super::workspace::WorktreeKind;
use std::time::Instant;

// ════════════════════════════════════════════════════════════════════
//  Conversation intent structs（原 ConversationIntent enum 的 27 个 variant）
// ════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartChat {
    pub submission: String,
}

/// 仅追加一条用户消息回显块，不创建新的 chat/turn。
///
/// 用于 ask_user 应答、队列输入冲刷等「在已激活的对话回合内回显用户输入」的场景。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendUserMessage {
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObserveAssistantText {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObserveThinkingText {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompleteBlock {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObserveToolCallStart {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
    pub id: ToolCallId,
    pub provider_id: Option<String>,
    pub name: String,
    pub index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObserveToolCallUpdate {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
    pub id: ToolCallId,
    pub provider_id: Option<String>,
    pub name: String,
    pub index: usize,
    pub arguments: Option<String>,
    pub status: ToolCallStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObserveToolResult {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
    pub id: ToolCallId,
    pub provider_id: String,
    pub tool_name: String,
    pub output: String,
    pub content: serde_json::Value,
    pub is_error: bool,
    pub image_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendSystemMessage {
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendHookNotice {
    pub content: HookNoticeContent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendError {
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueSubmission {
    pub input_id: sdk::InputId,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClearQueuedSubmissionById {
    pub input_id: sdk::InputId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClearAllQueuedSubmissions;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordAgentProgress {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
    pub tool_id: ToolCallId,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShowAskUserBatch {
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
pub struct CompleteChat {
    pub chat_id: ChatId,
    pub turn_id: ChatTurnId,
}

// ════════════════════════════════════════════════════════════════════
//  Runtime intent structs（原 RuntimeIntent enum 的 14 个 variant，
//  排除 SetSpinnerPhase / StopSpinner —— 它们的功能已被其他 intent 附带维护）
// ════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq)]
pub struct SetProviderModel {
    pub provider: Option<String>,
    pub model_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateWorkspace {
    pub cwd: String,
    pub worktree: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSnapshotReceived {
    pub path_base: Option<String>,
    pub workspace_root: Option<String>,
    pub branch: Option<String>,
    pub kind: WorktreeKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecordUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub last_input_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SetContextSize(pub u64);

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateLastInputTokens(pub u64);

#[derive(Clone, Debug, PartialEq)]
pub struct RecordLiveTps {
    pub tps: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateTaskStatus {
    pub total: usize,
    pub completed: usize,
    pub in_progress: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StartProcessingJob {
    pub id: String,
    pub chat_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FinishProcessingJob {
    pub id: String,
    pub success: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateTaskLines(pub Vec<String>);

#[derive(Clone, Debug, PartialEq)]
pub struct SetStatusNotice(pub StatusNotice);

#[derive(Clone, Debug, PartialEq)]
pub struct SetTransientStatusNotice {
    pub notice: StatusNotice,
    pub expires_at: Instant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SetThinking(pub bool);

#[derive(Clone, Debug, PartialEq)]
pub struct SetGraphPhase(pub Option<String>);

#[derive(Clone, Debug, PartialEq)]
pub struct SetCompactProgress {
    pub stage: String,
    pub current: Option<u32>,
    pub total: Option<u32>,
}

// ════════════════════════════════════════════════════════════════════
//  传输容器 enum
// ════════════════════════════════════════════════════════════════════

#[derive(Clone, Debug, PartialEq)]
pub enum ConversationIntent {
    // ── 原 conversation variants ──
    StartChat(StartChat),
    AppendUserMessage(AppendUserMessage),
    ObserveAssistantText(ObserveAssistantText),
    ObserveThinkingText(ObserveThinkingText),
    CompleteBlock(CompleteBlock),
    ObserveToolCallStart(ObserveToolCallStart),
    ObserveToolCallUpdate(ObserveToolCallUpdate),
    ObserveToolResult(ObserveToolResult),
    AppendSystemMessage(AppendSystemMessage),
    AppendHookNotice(AppendHookNotice),
    AppendError(AppendError),
    QueueSubmission(QueueSubmission),
    ClearQueuedSubmissionById(ClearQueuedSubmissionById),
    ClearAllQueuedSubmissions(ClearAllQueuedSubmissions),
    RecordAgentProgress(RecordAgentProgress),
    ShowAskUserBatch(ShowAskUserBatch),
    AnswerCurrentAskUser(AnswerCurrentAskUser),
    SetAskUserCursor(SetAskUserCursor),
    ToggleAskUserSelected(ToggleAskUserSelected),
    SetAskUserChatInput(SetAskUserChatInput),
    AppendAskUserChatChar(AppendAskUserChatChar),
    DeleteAskUserChatChar(DeleteAskUserChatChar),
    NavigateAskUserTo(NavigateAskUserTo),
    SetAskUserConfirmCursor(SetAskUserConfirmCursor),
    ConfirmAskUserBatch(ConfirmAskUserBatch),
    DismissAskUserBatch(DismissAskUserBatch),
    CompleteChat(CompleteChat),
    // ── 原 runtime variants ──
    SetProviderModel(SetProviderModel),
    UpdateWorkspace(UpdateWorkspace),
    WorkspaceSnapshotReceived(WorkspaceSnapshotReceived),
    RecordUsage(RecordUsage),
    SetContextSize(SetContextSize),
    UpdateLastInputTokens(UpdateLastInputTokens),
    RecordLiveTps(RecordLiveTps),
    UpdateTaskStatus(UpdateTaskStatus),
    StartProcessingJob(StartProcessingJob),
    FinishProcessingJob(FinishProcessingJob),
    UpdateTaskLines(UpdateTaskLines),
    SetStatusNotice(SetStatusNotice),
    SetTransientStatusNotice(SetTransientStatusNotice),
    SetThinking(SetThinking),
    SetGraphPhase(SetGraphPhase),
    SetCompactProgress(SetCompactProgress),
}

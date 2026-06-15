use super::block::HookNoticeContent;
use super::ids::{ChatId, ChatTurnId, ToolCallId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationIntent {
    StartChat {
        submission: String,
    },
    /// 仅追加一条用户消息回显块，不创建新的 chat/turn。
    ///
    /// 用于 ask_user 应答、队列输入冲刷等「在已激活的对话回合内回显用户输入」的场景，
    /// 此时不能用 `StartChat`（会新开 chat 并重置 active_chat_id，破坏在途工具绑定）。
    AppendUserMessage {
        text: String,
    },
    ObserveAssistantText {
        chat_id: ChatId,
        turn_id: ChatTurnId,
        text: String,
    },
    ObserveThinkingText {
        chat_id: ChatId,
        turn_id: ChatTurnId,
        text: String,
    },
    CompleteBlock {
        chat_id: ChatId,
        turn_id: ChatTurnId,
    },
    ObserveToolCallStart {
        chat_id: ChatId,
        turn_id: ChatTurnId,
        id: ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
    },
    ObserveToolCallUpdate {
        chat_id: ChatId,
        turn_id: ChatTurnId,
        id: ToolCallId,
        provider_id: Option<String>,
        name: String,
        index: usize,
        arguments: Option<String>,
        status: super::tool_call::ToolCallStatus,
    },
    ObserveToolResult {
        chat_id: ChatId,
        turn_id: ChatTurnId,
        id: ToolCallId,
        provider_id: String,
        tool_name: String,
        output: String,
        content: serde_json::Value,
        is_error: bool,
        image_count: usize,
    },
    AppendSystemMessage {
        text: String,
    },
    AppendHookNotice {
        content: HookNoticeContent,
    },
    AppendError {
        text: String,
    },
    QueueSubmission {
        text: String,
    },
    ClearQueuedSubmissions,
    RecordAgentProgress {
        chat_id: ChatId,
        turn_id: ChatTurnId,
        tool_id: ToolCallId,
        message: String,
    },
    /// 显示 AskUserBatch 交互块（批量问题）。
    ShowAskUserBatch {
        slots: Vec<super::block::AskUserSlot>,
    },
    /// 回答当前激活问题，自动前进到下一题或进入确认页。
    AnswerCurrentAskUser {
        answer: String,
    },
    /// 更新当前激活问题的选项光标。
    SetAskUserCursor {
        cursor: usize,
    },
    /// 切换当前激活问题中某选项的勾选状态。
    ToggleAskUserSelected {
        index: usize,
    },
    /// 设置当前激活问题是否处于 Type something 子态。
    SetAskUserChatInput {
        active: bool,
    },
    /// 追加字符到 Type something 输入框。
    AppendAskUserChatChar {
        ch: char,
    },
    /// 删除 Type something 输入框末尾字符。
    DeleteAskUserChatChar,
    /// 确认页：导航到某项（重新作答）。
    NavigateAskUserTo {
        index: usize,
    },
    /// 更新确认页导航光标。
    SetAskUserConfirmCursor {
        cursor: usize,
    },
    /// 确认提交所有答案。
    ConfirmAskUserBatch,
    /// 取消整个 batch（回传空答案）。
    DismissAskUserBatch,
    CompleteChat {
        chat_id: ChatId,
        turn_id: ChatTurnId,
    },
}

//! 交互式批量提问状态
//!
//! 导航高亮的可变状态（cursor/selected/chat_input_active）由
//! `ConversationModel` 的 `AskUserBatch` 块管理；本结构仅保留
//! 应答回传所需的 reply_tx 和原始问题列表。

/// Built-in options appended after LLM options in AskUserQuestion.
pub(crate) const BUILTIN_OPTION_CHAT: &str = "Type something...";

/// 批量 AskUserQuestion 交互状态。
pub(crate) struct AskUserState {
    /// 回传所有答案（顺序与 items 一致）。
    pub reply_tx: tokio::sync::oneshot::Sender<sdk::AskUserReply>,
    /// 原始问题列表（用于构建 AskUserSlot 和回传时取 default）。
    pub items: Vec<sdk::AskUserQuestionItem>,
}

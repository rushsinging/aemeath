//! 交互式批量提问状态
//!
//! #944 5B: Legacy AskUser reply_tx bridge removed. AskUserState is now dead
//! code (never constructed); retained until key.rs routing is cleaned up.
#![allow(dead_code)]

/// Built-in options appended after LLM options in AskUserQuestion.
pub(crate) const BUILTIN_OPTION_CHAT: &str = "Type something...";

/// 批量 AskUserQuestion 交互状态。
pub(crate) struct AskUserState {
    /// 回传所有答案（顺序与 items 一致）。
    pub reply_tx: tokio::sync::oneshot::Sender<sdk::AskUserReply>,
    /// 原始问题列表（用于构建 AskUserSlot 和回传时取 default）。
    pub items: Vec<sdk::AskUserQuestionItem>,
}

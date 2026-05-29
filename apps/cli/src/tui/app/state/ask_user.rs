//! 交互式选项提问状态
//!
//! 选项导航的可变状态（cursor/selected/chat_input_active）已迁入
//! `ConversationModel` 的 `AskUser` 块，作为渲染与导航高亮的单一真相；
//! 本结构仅保留应答回传所需的静态元数据与 reply_tx。

/// Built-in options appended after LLM options in AskUserQuestion.
pub(crate) const BUILTIN_OPTION_ALL: &str = "All of the above";
pub(crate) const BUILTIN_OPTION_CHAT: &str = "Chat about this...";

/// State for interactive AskUserQuestion option selection
pub(crate) struct AskUserState {
    pub reply_tx: tokio::sync::oneshot::Sender<String>,
    /// All options shown to user: LLM options + built-in options.
    pub options: Vec<String>,
    /// Number of LLM-provided options (built-in options start at this index).
    pub llm_option_count: usize,
    pub multi_select: bool,
    /// Whether free-text input is allowed
    #[allow(dead_code)]
    pub allow_free_input: bool,
}

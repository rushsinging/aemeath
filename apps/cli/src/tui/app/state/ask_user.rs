//! 交互式选项提问状态

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
    pub cursor: usize,
    pub multi_select: bool,
    pub selected: Vec<bool>,
    /// Ranges in output_area.lines for each rendered option row.
    pub option_line_ranges: Vec<std::ops::Range<usize>>,
    /// Whether free-text input is allowed
    #[allow(dead_code)]
    pub allow_free_input: bool,
    /// When true, user is typing free-text answer via "Chat about this...".
    pub chat_input_active: bool,
}

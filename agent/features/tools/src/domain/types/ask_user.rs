//! Typed result for the `ask_user` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Typed answer returned after Runtime resumes the suspended AskUser tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AskUserQuestionResult {
    pub text: String,
}

/// Typed input for the `ask_user` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AskUserQuestionInput {
    /// The question prompt only. Do not include selectable choices here; put choices in options.
    ///
    /// Markdown is encouraged: use short paragraphs, `- ` bullet lists, and line breaks to
    /// structure complex confirmations. The TUI renders the full `question` as the body of
    /// the prompt area — it is never truncated or used as a header — so prefer multi-line
    /// Markdown over a single long sentence when the context warrants detail.
    pub question: String,
    /// Optional list of predefined answer choices. Each choice MUST be one separate array item — either a plain string or an object { title, description }. Do not combine choices into one string or embed them in question.
    pub options: Option<Vec<Value>>,
    /// If true, the user may select more than one predefined choice
    pub multi_select: Option<bool>,
    /// If true, user can provide any answer (not limited to options). Defaults to true. When
    /// predefined options are present, the system adds a built-in "Type something..." free-text
    /// entry; do not include that entry yourself in options. Set false only when answers must be
    /// restricted to options.
    pub allow_free_input: Option<bool>,
    /// Optional default answer if user skips
    pub default: Option<String>,
}

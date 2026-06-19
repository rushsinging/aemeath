//! Typed result for the `ask_user` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Typed result returned by the `ask_user` tool.
///
/// `options` is the list of answer choices presented to the user.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AskUserQuestionResult {
    pub question_type: String,
    pub options: Vec<Value>,
    pub allow_free_input: bool,
}

/// Typed input for the `ask_user` tool.
///
/// build.rs 由本 struct 生成 `input_schema`（字段 `///` 注释即 LLM 看到的参数描述）。
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AskUserQuestionInput {
    /// The question prompt only. Do not include selectable choices here; put choices in options.
    pub question: String,
    /// Optional list of predefined answer choices. Each choice MUST be one separate array item — either a plain string or an object { title, description }. Do not combine choices into one string or embed them in question.
    pub options: Option<Vec<Value>>,
    /// If true, user can provide any answer (not limited to options)
    pub allow_free_input: Option<bool>,
    /// Optional default answer if user skips
    pub default: Option<String>,
}

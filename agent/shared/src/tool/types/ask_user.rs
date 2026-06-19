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

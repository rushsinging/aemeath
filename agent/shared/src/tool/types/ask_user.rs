//! Typed result for the `ask_user` tool (issue #273 core tool).

use super::support::AskOption;
use serde::{Deserialize, Serialize};

/// Typed result returned by the `ask_user` tool.
///
/// `options` is the list of answer choices presented to the user; each
/// option uses `AskOption` (renamed from the bare `Option` to avoid shadowing
/// `std::option::Option`).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AskUserQuestionResult {
    pub question_type: String,
    pub question: String,
    pub options: Vec<AskOption>,
}
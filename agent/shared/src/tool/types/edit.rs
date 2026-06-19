//! Typed result for the `edit` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;
use std::path::PathBuf;

/// Typed result returned by the `edit` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct EditResult {
    pub file_path: PathBuf,
    pub occurrences: usize,
    pub diff: String,
}
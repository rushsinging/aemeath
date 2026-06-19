//! Typed result for the `glob` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;
use std::path::PathBuf;

/// Typed result returned by the `glob` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct GlobResult {
    pub files: Vec<PathBuf>,
    pub match_count: usize,
}
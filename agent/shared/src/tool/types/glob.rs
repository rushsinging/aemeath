//! Typed result for the `glob` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Typed result returned by the `glob` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {files: array, count: integer}
pub struct GlobResult {
    pub files: Vec<PathBuf>,
    pub match_count: usize,
}
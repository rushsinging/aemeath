//! Typed result for the `grep` tool (issue #273 core tool).

use super::support::Match;
use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `grep` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct GrepResult {
    pub matches: Vec<Match>,
    pub match_count: usize,
    pub truncated: bool,
}
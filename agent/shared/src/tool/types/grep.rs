//! Typed result for the `grep` tool (issue #273 core tool).

use super::support::Match;
use serde::{Deserialize, Serialize};

/// Typed result returned by the `grep` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {matches: array, total_matches: integer, query: string}
pub struct GrepResult {
    pub matches: Vec<Match>,
    pub total_matches: u64,
    pub query: String,
}

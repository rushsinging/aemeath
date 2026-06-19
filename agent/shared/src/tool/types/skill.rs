//! Typed result for the `skill` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `skill` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {name: string, path: string}
pub struct SkillResult {
    pub name: String,
    pub output: String,
}
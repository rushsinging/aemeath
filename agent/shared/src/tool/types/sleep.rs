//! Typed result for the `sleep` tool (non-core tool).

use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `sleep` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct SleepResult {
    pub slept_ms: u64,
}
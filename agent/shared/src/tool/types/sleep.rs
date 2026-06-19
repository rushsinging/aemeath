//! Typed result for the `sleep` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `sleep` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {duration_ms: integer}
pub struct SleepResult {
    pub duration_ms: u64,
}
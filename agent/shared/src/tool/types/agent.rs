//! Typed result for the `agent` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `agent` tool (sub-agent dispatch).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct AgentResult {
    pub agent_id: String,
    pub output: String,
}
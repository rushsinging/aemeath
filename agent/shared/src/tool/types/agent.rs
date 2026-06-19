//! Typed result for the `agent` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `agent` tool (sub-agent dispatch).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {agent_id: string, output: string}
pub struct AgentResult {
    pub agent_id: String,
    pub output: String,
}
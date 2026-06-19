//! Typed result for the `plan_mode` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `plan_mode` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PlanModeResult {
    pub mode: String,
    pub content: String,
}
//! Typed result for the `sleep` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `sleep` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SleepResult {
    pub slept_ms: u64,
}
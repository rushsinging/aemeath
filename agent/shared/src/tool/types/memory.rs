//! Typed result for the `memory` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `memory` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct MemoryResult {
    pub action: String,
}

//! Typed result for the `brief` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `brief` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BriefResult {
    pub summary: String,
}

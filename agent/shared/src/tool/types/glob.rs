//! Typed result for the `glob` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `glob` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct GlobResult {
    pub files: Vec<String>,
    pub count: u64,
}

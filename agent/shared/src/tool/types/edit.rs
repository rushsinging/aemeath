//! Typed result for the `edit` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `edit` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EditResult {
    pub file_path: String,
    pub replacements_made: u64,
    pub dry_run: bool,
}

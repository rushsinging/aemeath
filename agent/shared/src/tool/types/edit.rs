//! Typed result for the `edit` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Typed result returned by the `edit` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {file_path: string, replacements_made: integer, dry_run: boolean}
pub struct EditResult {
    pub file_path: PathBuf,
    pub occurrences: usize,
    pub diff: String,
}
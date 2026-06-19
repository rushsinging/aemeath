//! Typed result for the `edit` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Typed result returned by the `edit` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EditResult {
    pub file_path: PathBuf,
    pub occurrences: usize,
    pub diff: String,
}
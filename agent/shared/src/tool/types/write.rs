//! Typed result for the `write` tool (issue #273 core tool).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Typed result returned by the `write` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {file_path: string, bytes_written: integer}
pub struct WriteResult {
    pub file_path: PathBuf,
    pub bytes_written: u64,
}
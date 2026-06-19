//! Typed result for the `lsp` tool (non-core tool).

use serde::{Deserialize, Serialize};

/// Typed result returned by the `lsp` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LspResult {
    pub output: String,
}

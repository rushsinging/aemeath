//! Typed result for the `lsp` tool (non-core tool).

use super::support::Diagnostic;
use serde::{Deserialize, Serialize};

/// Typed result returned by the `lsp` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
// tool_schema: {output: string}
pub struct LspResult {
    pub diagnostics: Vec<Diagnostic>,
}
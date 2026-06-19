//! Typed result for the `lsp` tool (non-core tool).

use super::support::Diagnostic;
use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `lsp` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct LspResult {
    pub diagnostics: Vec<Diagnostic>,
}
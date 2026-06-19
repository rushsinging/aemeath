//! Small support types referenced by more than one tool's result struct.
//! Lives in `share` (rather than in any single feature crate) so that the
//! result structs in `share::tool::types::*` can be `use`d from any feature
//! without inverting DDD boundaries.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

/// A single match reported by the `grep` tool.
///
/// Field names mirror the `GrepResult::matches` JSON shape that TUI consumers
/// already understand, so existing TUI code keeps working once the tool's
/// payload is typed as `GrepResult`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Match {
    pub file_path: PathBuf,
    pub line_number: u64,
    pub line: String,
}

/// A single answer option presented by the `ask_user` tool.
///
/// Named `AskOption` rather than `Option` to avoid shadowing
/// `std::option::Option` in field type positions.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AskOption {
    pub label: String,
    pub description: String,
    pub preview: Option<String>,
}

/// A single LSP diagnostic entry returned by the `lsp` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Diagnostic {
    pub file_path: PathBuf,
    pub line: u64,
    pub column: u64,
    pub severity: String,
    pub message: String,
}

/// A single web search hit returned by the `web_search` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Typed view of an MCP resource, mirroring the `McpResource` shape
/// defined in the `list_mcp_resources` tool.
///
/// Defined here in `share` so that `ListMcpResourcesResult` and
/// `ReadMcpResourceResult` can stay typed without depending on the
/// `tools` feature (which would invert DDD).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub mime_type: Option<String>,
    pub description: Option<String>,
    pub server: String,
}

/// Content body of a single MCP resource read by the `read_mcp_resource` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ResourceContent {
    pub uri: String,
    pub mime_type: Option<String>,
    pub text: Option<String>,
    pub blob_saved_to: Option<String>,
}

/// A single (key, value) configuration entry returned by the `config` tool.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ConfigEntry {
    pub key: String,
    pub value: Value,
}

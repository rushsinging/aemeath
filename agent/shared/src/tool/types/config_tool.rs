//! Typed result for the `config` tool (non-core tool).

use super::support::ConfigEntry;
use serde::{Deserialize, Serialize};
use tool_schema_macros::ToolSchema;

/// Typed result returned by the `config` tool.
///
/// `entries` lists the (key, value) pairs that the operation touched
/// (read or write).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, ToolSchema)]
pub struct ConfigResult {
    pub entries: Vec<ConfigEntry>,
}
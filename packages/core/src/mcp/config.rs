use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;

/// MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub transport: Option<McpTransportKind>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpTransportKind {
    Stdio,
    Sse,
    StreamableHttp,
}

impl fmt::Display for McpTransportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            McpTransportKind::Stdio => "stdio",
            McpTransportKind::Sse => "sse",
            McpTransportKind::StreamableHttp => "streamable_http",
        };
        f.write_str(name)
    }
}

impl McpServerConfig {
    /// Resolve the configured transport.
    ///
    /// An explicit `transport` takes precedence. Otherwise, stdio is selected
    /// when `command` is present, and streamable HTTP is selected when only
    /// `url` is present.
    pub fn transport_kind(&self) -> Result<McpTransportKind, String> {
        if let Some(kind) = self.transport {
            return Ok(kind);
        }
        if self
            .command
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
        {
            return Ok(McpTransportKind::Stdio);
        }
        if self.url.as_deref().is_some_and(|s| !s.trim().is_empty()) {
            return Ok(McpTransportKind::StreamableHttp);
        }
        Err("MCP server config must define either command or url".to_string())
    }
}

/// An MCP tool definition received from a server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
}

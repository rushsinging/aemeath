use crate::adapters::mcp::{McpClient, McpServerConfig, McpToolDef};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    /// Server is being initialized
    Initializing,
    /// Server is connected and ready
    Connected,
    /// Server connection failed
    Failed,
    /// Server is disabled
    Disabled,
    /// Server is reconnecting
    Reconnecting,
}

/// MCP server connection info
#[derive(Clone)]
pub struct McpServerConnection {
    /// Server name
    pub name: String,
    /// Server configuration
    pub config: McpServerConfig,
    /// Connection state
    pub state: ConnectionState,
    /// Connected client (if connected)
    pub client: Option<Arc<Mutex<McpClient>>>,
    /// Available tools from this server
    pub tools: Vec<McpToolDef>,
    /// Error message if failed
    pub error: Option<String>,
    /// Whether server supports resources
    pub supports_resources: bool,
}

impl McpServerConnection {
    /// Create an initializing MCP server connection.
    pub fn initializing(name: String, config: McpServerConfig) -> Self {
        Self {
            name,
            config,
            state: ConnectionState::Initializing,
            client: None,
            tools: Vec::new(),
            error: None,
            supports_resources: false,
        }
    }

    /// Create a failed MCP server connection with an error message.
    pub fn failed(name: String, config: McpServerConfig, error: String) -> Self {
        Self {
            name,
            config,
            state: ConnectionState::Failed,
            client: None,
            tools: Vec::new(),
            error: Some(error),
            supports_resources: false,
        }
    }
}

fn default_auto_connect() -> bool {
    true
}

fn default_auto_reconnect() -> bool {
    true
}

fn default_reconnect_delay_seconds() -> u64 {
    5
}

fn default_max_reconnect_attempts() -> u32 {
    3
}

fn default_health_check_interval_seconds() -> u64 {
    30
}

fn default_max_tool_response_bytes() -> usize {
    crate::adapters::mcp::DEFAULT_MAX_TOOL_RESPONSE_BYTES
}

/// MCP connection manager configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpManagerConfig {
    /// Server configurations
    #[serde(default)]
    pub servers: HashMap<String, McpServerConfig>,
    /// Auto-connect on startup
    #[serde(default = "default_auto_connect")]
    pub auto_connect: bool,
    /// Reconnect on failure
    #[serde(default = "default_auto_reconnect")]
    pub auto_reconnect: bool,
    /// Reconnect delay in seconds
    #[serde(default = "default_reconnect_delay_seconds")]
    pub reconnect_delay_seconds: u64,
    /// Max reconnect attempts
    #[serde(default = "default_max_reconnect_attempts")]
    pub max_reconnect_attempts: u32,
    /// Health check interval in seconds
    #[serde(default = "default_health_check_interval_seconds")]
    pub health_check_interval_seconds: u64,
    /// Maximum MCP tool response size in bytes
    #[serde(default = "default_max_tool_response_bytes")]
    pub max_tool_response_bytes: usize,
}

impl Default for McpManagerConfig {
    fn default() -> Self {
        Self {
            servers: HashMap::new(),
            auto_connect: true,
            auto_reconnect: true,
            reconnect_delay_seconds: 5,
            max_reconnect_attempts: 3,
            health_check_interval_seconds: 30,
            max_tool_response_bytes: crate::adapters::mcp::DEFAULT_MAX_TOOL_RESPONSE_BYTES,
        }
    }
}

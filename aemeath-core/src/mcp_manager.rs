//! MCP Connection Manager
//!
//! Manages connections to multiple MCP servers, providing:
//! - Server configuration loading
//! - Connection lifecycle management
//! - Tool discovery and registration
//! - Resource discovery
//! - Reconnection handling

use crate::mcp::{McpClient, McpServerConfig, McpToolDef};
use crate::tool::{Tool, ToolContext, ToolRegistry, ToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// MCP server connection state
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
    crate::mcp::DEFAULT_MAX_TOOL_RESPONSE_BYTES
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
            max_tool_response_bytes: crate::mcp::DEFAULT_MAX_TOOL_RESPONSE_BYTES,
        }
    }
}

/// Tool list changes between two MCP tool snapshots.
#[derive(Debug, Clone)]
pub struct ToolListDiff {
    /// Tools present in the new list but absent from the old list.
    pub added: Vec<McpToolDef>,
    /// Tool names present in the old list but absent from the new list.
    pub removed: Vec<String>,
    /// Tools whose description or input schema changed.
    pub changed: Vec<McpToolDef>,
}

/// Compute added, removed, and changed MCP tools by tool name.
pub fn diff_tools(old: &[McpToolDef], new: &[McpToolDef]) -> ToolListDiff {
    let old_by_name: HashMap<&str, &McpToolDef> =
        old.iter().map(|tool| (tool.name.as_str(), tool)).collect();
    let new_by_name: HashMap<&str, &McpToolDef> =
        new.iter().map(|tool| (tool.name.as_str(), tool)).collect();

    let added = new
        .iter()
        .filter(|tool| !old_by_name.contains_key(tool.name.as_str()))
        .cloned()
        .collect();

    let removed = old
        .iter()
        .filter(|tool| !new_by_name.contains_key(tool.name.as_str()))
        .map(|tool| tool.name.clone())
        .collect();

    let changed = new
        .iter()
        .filter(|new_tool| {
            old_by_name
                .get(new_tool.name.as_str())
                .is_some_and(|old_tool| {
                    old_tool.description != new_tool.description
                        || old_tool.input_schema != new_tool.input_schema
                })
        })
        .cloned()
        .collect();

    ToolListDiff {
        added,
        removed,
        changed,
    }
}

/// Build the registry-qualified name for an MCP tool.
pub fn qualified_tool_name(server: &str, tool: &str) -> String {
    format!("mcp__{}__{}", server, tool)
}

/// Build registry-qualified names for tools removed from an MCP server.
pub fn removed_qualified_tool_names(server: &str, removed: &[String]) -> Vec<String> {
    removed
        .iter()
        .map(|tool| qualified_tool_name(server, tool))
        .collect()
}

/// MCP connection manager
pub struct McpConnectionManager {
    /// Configuration
    config: McpManagerConfig,
    /// Server connections
    connections: Arc<Mutex<HashMap<String, McpServerConnection>>>,
    /// All discovered tools (server_name -> tools)
    discovered_tools: Arc<Mutex<HashMap<String, Vec<McpToolDef>>>>,
}

impl McpConnectionManager {
    /// Create a new connection manager
    pub fn new(config: McpManagerConfig) -> Self {
        Self {
            config,
            connections: Arc::new(Mutex::new(HashMap::new())),
            discovered_tools: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create with default configuration
    pub fn with_servers(servers: HashMap<String, McpServerConfig>) -> Self {
        let config = McpManagerConfig {
            servers,
            ..Default::default()
        };
        Self::new(config)
    }

    /// Load configuration from JSON file
    pub fn load_from_file(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let config: McpManagerConfig =
            serde_json::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?;

        Ok(Self::new(config))
    }

    /// Initialize all configured servers
    pub async fn initialize(&self) -> Result<(), String> {
        let mut connections = self.connections.lock().await;

        for (name, config) in &self.config.servers {
            let connection = McpServerConnection::initializing(name.clone(), config.clone());
            connections.insert(name.clone(), connection);
        }

        Ok(())
    }

    /// Connect to a specific server
    pub async fn connect_server(&self, name: &str) -> Result<McpServerConnection, String> {
        let connection = {
            let connections = self.connections.lock().await;
            connections
                .get(name)
                .cloned()
                .ok_or_else(|| format!("Server '{}' not configured", name))?
        };

        // Attempt connection
        let client = McpClient::connect(name, &connection.config).await;

        let mut connections = self.connections.lock().await;

        match client {
            Ok(client) => {
                // Discover tools
                let tools = client.list_tools().await.unwrap_or_default();

                // Check for resource support
                let supports_resources = self.check_resource_support(&client).await;

                let client_arc = Arc::new(Mutex::new(client));

                let updated = McpServerConnection {
                    name: name.to_string(),
                    config: connection.config.clone(),
                    state: ConnectionState::Connected,
                    client: Some(client_arc.clone()),
                    tools: tools.clone(),
                    error: None,
                    supports_resources,
                };

                connections.insert(name.to_string(), updated.clone());

                // Store discovered tools
                let mut discovered = self.discovered_tools.lock().await;
                discovered.insert(name.to_string(), tools);

                Ok(updated)
            }
            Err(e) => {
                let updated = McpServerConnection::failed(
                    name.to_string(),
                    connection.config.clone(),
                    e.clone(),
                );

                connections.insert(name.to_string(), updated.clone());
                Err(e)
            }
        }
    }

    /// Connect all configured servers
    pub async fn connect_all(&self) -> HashMap<String, Result<McpServerConnection, String>> {
        let mut results = HashMap::new();

        for name in self.config.servers.keys() {
            let result = self.connect_server(name).await;
            results.insert(name.clone(), result);
        }

        results
    }

    /// Disconnect a specific server
    pub async fn disconnect_server(&self, name: &str) -> Result<(), String> {
        let mut connections = self.connections.lock().await;

        if let Some(connection) = connections.get_mut(name) {
            if let Some(client) = &connection.client {
                let mut client = client.lock().await;
                client.shutdown().await;
            }

            connection.state = ConnectionState::Disabled;
            connection.client = None;
            connection.tools.clear();
        }

        Ok(())
    }

    /// Reconnect a server (useful after failure)
    pub async fn reconnect_server(&self, name: &str) -> Result<McpServerConnection, String> {
        let mut connections = self.connections.lock().await;

        if let Some(connection) = connections.get_mut(name) {
            connection.state = ConnectionState::Reconnecting;
            connection.error = None;
        }

        drop(connections);

        self.connect_server(name).await
    }

    /// Toggle server enabled/disabled
    pub async fn toggle_server(&self, name: &str) -> Result<(), String> {
        // Read state while holding the lock, then release before calling
        // other methods that need the same lock.
        let should_enable = {
            let connections = self.connections.lock().await;
            let connection = connections.get(name).cloned();
            match connection {
                None => return Err(format!("Server '{}' not found", name)),
                Some(c) => c.state == ConnectionState::Disabled,
            }
        }; // lock released here

        if should_enable {
            self.reconnect_server(name).await?;
        } else {
            self.disconnect_server(name).await?;
        }

        Ok(())
    }

    /// Get a server connection
    pub async fn get_server(&self, name: &str) -> Option<McpServerConnection> {
        let connections = self.connections.lock().await;
        connections.get(name).cloned()
    }

    /// Get all server connections
    pub async fn get_all_servers(&self) -> Vec<McpServerConnection> {
        let connections = self.connections.lock().await;
        connections.values().cloned().collect()
    }

    /// Get connected servers
    pub async fn get_connected_servers(&self) -> Vec<McpServerConnection> {
        let connections = self.connections.lock().await;
        connections
            .values()
            .filter(|c| c.state == ConnectionState::Connected)
            .cloned()
            .collect()
    }

    /// Get all discovered tools
    pub async fn get_all_tools(&self) -> Vec<(String, McpToolDef)> {
        let discovered = self.discovered_tools.lock().await;
        discovered
            .iter()
            .flat_map(|(server, tools)| tools.iter().map(|t| (server.clone(), t.clone())))
            .collect()
    }

    /// Register MCP tools into a tool registry
    pub async fn register_tools(&self, registry: &mut ToolRegistry) {
        let connections = self.connections.lock().await;

        for connection in connections.values() {
            if connection.state != ConnectionState::Connected {
                continue;
            }

            if let Some(client_arc) = &connection.client {
                for tool_def in &connection.tools {
                    // Create qualified name: mcp__server__tool
                    let qualified_name = qualified_tool_name(&connection.name, &tool_def.name);

                    let mcp_tool = McpToolWrapper {
                        tool_name: tool_def.name.clone(),
                        qualified_name: qualified_name.clone(),
                        description: tool_def.description.clone(),
                        schema: tool_def.input_schema.clone(),
                        client: client_arc.clone(),
                    };

                    registry.register(Box::new(mcp_tool));
                }
            }
        }
    }

    /// Check if server supports resources
    async fn check_resource_support(&self, client: &McpClient) -> bool {
        // Try to list resources, if successful then supports resources
        client.send_request("resources/list", None).await.is_ok()
    }

    /// Shutdown all connections
    pub async fn shutdown(&self) {
        let mut connections = self.connections.lock().await;

        for connection in connections.values_mut() {
            if let Some(client) = &connection.client {
                let mut client = client.lock().await;
                client.shutdown().await;
            }
            connection.state = ConnectionState::Disabled;
            connection.client = None;
        }
    }
}

/// Wrapper for MCP tool to implement Tool trait
struct McpToolWrapper {
    tool_name: String,
    qualified_name: String,
    description: String,
    schema: Value,
    client: Arc<Mutex<McpClient>>,
}

/// Validate MCP tool input against JSON Schema
fn validate_mcp_input(input: &Value, schema: &Value) -> Result<(), String> {
    // Basic schema validation - check required fields
    if let Some(obj) = input.as_object() {
        if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
            // Check required fields
            if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
                for field in required {
                    if let Some(field_name) = field.as_str() {
                        if !obj.contains_key(field_name) {
                            return Err(format!("Missing required field: {}", field_name));
                        }
                    }
                }
            }

            // Check field types
            for (key, value) in obj {
                if let Some(prop_schema) = props.get(key) {
                    let expected_type = prop_schema
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("any");
                    let actual_type = match value {
                        Value::Null => "null",
                        Value::Bool(_) => "boolean",
                        Value::Number(_) => "number",
                        Value::String(_) => "string",
                        Value::Array(_) => "array",
                        Value::Object(_) => "object",
                    };
                    // Allow number to match integer type loosely
                    if expected_type != "any"
                        && expected_type != actual_type
                        && !(expected_type == "integer" && actual_type == "number")
                    {
                        return Err(format!(
                            "Type mismatch for field '{}': expected {}, got {}",
                            key, expected_type, actual_type
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

#[async_trait]
impl Tool for McpToolWrapper {
    fn name(&self) -> &str {
        &self.qualified_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.schema.clone()
    }

    fn is_read_only(&self) -> bool {
        // MCP tools are generally not read-only unless specified
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        // MCP tools are generally concurrency safe
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        // Validate input against schema before calling MCP tool
        if let Err(e) = validate_mcp_input(&input, &self.schema) {
            log::warn!("MCP tool {} input validation failed: {}", self.tool_name, e);
            return ToolResult::error(format!("Invalid input for {}: {}", self.tool_name, e));
        }

        let client = self.client.lock().await;
        match client.call_tool(&self.tool_name, input).await {
            Ok(output) => ToolResult::success(crate::mcp::limit_tool_response(
                &output,
                crate::mcp::DEFAULT_MAX_TOOL_RESPONSE_BYTES,
            )),
            Err(e) => ToolResult::error(format!("MCP tool error: {}", e)),
        }
    }
}

/// Shared MCP connection manager
pub type SharedMcpManager = Arc<McpConnectionManager>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, description: &str) -> McpToolDef {
        McpToolDef {
            name: name.to_string(),
            description: description.to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                }
            }),
        }
    }

    fn server_config(command: Option<&str>) -> McpServerConfig {
        McpServerConfig {
            command: command.map(str::to_string),
            args: Vec::new(),
            env: HashMap::new(),
            url: None,
            headers: HashMap::new(),
            transport: None,
        }
    }

    #[test]
    fn test_diff_tools_detects_added_removed_and_changed() {
        let unchanged_old = tool("unchanged", "same");
        let unchanged_new = tool("unchanged", "same");
        let changed_old = tool("changed", "old description");
        let changed_new = tool("changed", "new description");
        let schema_changed_old = tool("schema_changed", "same description");
        let mut schema_changed_new = tool("schema_changed", "same description");
        schema_changed_new.input_schema = json!({
            "type": "object",
            "properties": {
                "count": { "type": "integer" }
            }
        });
        let removed = tool("removed", "removed description");
        let added = tool("added", "added description");

        let diff = diff_tools(
            &[unchanged_old, changed_old, schema_changed_old, removed],
            &[
                unchanged_new,
                changed_new.clone(),
                schema_changed_new.clone(),
                added.clone(),
            ],
        );

        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].name, added.name);
        assert_eq!(diff.removed, vec!["removed".to_string()]);
        assert_eq!(diff.changed.len(), 2);
        assert_eq!(diff.changed[0].name, changed_new.name);
        assert_eq!(diff.changed[0].description, changed_new.description);
        assert_eq!(diff.changed[1].name, schema_changed_new.name);
        assert_eq!(diff.changed[1].input_schema, schema_changed_new.input_schema);
    }

    #[test]
    fn test_qualified_tool_name_uses_double_separator() {
        assert_eq!(qualified_tool_name("server", "read"), "mcp__server__read");
    }

    #[test]
    fn test_tool_names_for_unregister_returns_removed_names() {
        let removed = vec!["read".to_string(), "write".to_string()];

        assert_eq!(
            removed_qualified_tool_names("demo", &removed),
            vec!["mcp__demo__read".to_string(), "mcp__demo__write".to_string()]
        );
    }

    #[test]
    fn test_mcp_manager_config_defaults_include_health_check() {
        let config = McpManagerConfig::default();

        assert!(config.servers.is_empty());
        assert!(config.auto_connect);
        assert!(config.auto_reconnect);
        assert_eq!(config.reconnect_delay_seconds, 5);
        assert_eq!(config.max_reconnect_attempts, 3);
        assert_eq!(config.health_check_interval_seconds, 30);
        assert_eq!(
            config.max_tool_response_bytes,
            crate::mcp::DEFAULT_MAX_TOOL_RESPONSE_BYTES
        );
    }

    #[test]
    fn test_mcp_manager_config_deserializes_empty_object_with_defaults() {
        let config: McpManagerConfig = serde_json::from_str("{}").unwrap();

        assert!(config.servers.is_empty());
        assert!(config.auto_connect);
        assert!(config.auto_reconnect);
        assert_eq!(config.reconnect_delay_seconds, 5);
        assert_eq!(config.max_reconnect_attempts, 3);
        assert_eq!(config.health_check_interval_seconds, 30);
        assert_eq!(
            config.max_tool_response_bytes,
            crate::mcp::DEFAULT_MAX_TOOL_RESPONSE_BYTES
        );
    }

    #[test]
    fn test_connection_state_failed_stores_error() {
        let config = server_config(Some("/bin/example-mcp"));
        let connection = McpServerConnection::failed(
            "example".to_string(),
            config.clone(),
            "connection refused".to_string(),
        );

        assert_eq!(connection.name, "example");
        assert_eq!(connection.config.command, config.command);
        assert_eq!(connection.state, ConnectionState::Failed);
        assert_eq!(connection.error.as_deref(), Some("connection refused"));
        assert!(connection.client.is_none());
        assert!(connection.tools.is_empty());
        assert!(!connection.supports_resources);
    }
}

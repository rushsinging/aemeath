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
use crate::tools::mcp_tool::McpTool;
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
#[derive(Debug, Clone)]
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

/// MCP connection manager configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpManagerConfig {
    /// Server configurations
    pub servers: HashMap<String, McpServerConfig>,
    /// Auto-connect on startup
    pub auto_connect: bool,
    /// Reconnect on failure
    pub auto_reconnect: bool,
    /// Reconnect delay in seconds
    pub reconnect_delay_seconds: u64,
    /// Max reconnect attempts
    pub max_reconnect_attempts: u32,
}

impl Default for McpManagerConfig {
    fn default() -> Self {
        Self {
            servers: HashMap::new(),
            auto_connect: true,
            auto_reconnect: true,
            reconnect_delay_seconds: 5,
            max_reconnect_attempts: 3,
        }
    }
}

/// MCP connection manager
#[derive(Debug)]
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
        
        let config: McpManagerConfig = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse config: {}", e))?;
        
        Ok(Self::new(config))
    }

    /// Initialize all configured servers
    pub async fn initialize(&self) -> Result<(), String> {
        let mut connections = self.connections.lock().await;
        
        for (name, config) in &self.config.servers {
            let connection = McpServerConnection {
                name: name.clone(),
                config: config.clone(),
                state: ConnectionState::Initializing,
                client: None,
                tools: Vec::new(),
                error: None,
                supports_resources: false,
            };
            connections.insert(name.clone(), connection);
        }
        
        Ok(())
    }

    /// Connect to a specific server
    pub async fn connect_server(&self, name: &str) -> Result<McpServerConnection, String> {
        let connections = self.connections.lock().await;
        
        let connection = connections.get(name).cloned();
        if connection.is_none() {
            return Err(format!("Server '{}' not configured", name));
        }
        
        let connection = connection?;

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
                let updated = McpServerConnection {
                    name: name.to_string(),
                    config: connection.config.clone(),
                    state: ConnectionState::Failed,
                    client: None,
                    tools: Vec::new(),
                    error: Some(e.clone()),
                    supports_resources: false,
                };
                
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
        connections.values()
            .filter(|c| c.state == ConnectionState::Connected)
            .cloned()
            .collect()
    }

    /// Get all discovered tools
    pub async fn get_all_tools(&self) -> Vec<(String, McpToolDef)> {
        let discovered = self.discovered_tools.lock().await;
        discovered.iter()
            .flat_map(|(server, tools)| {
                tools.iter().map(|t| (server.clone(), t.clone()))
            })
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
                    let qualified_name = format!("mcp__{}__{}", connection.name, tool_def.name);
                    
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
                    let expected_type = prop_schema.get("type").and_then(|t| t.as_str()).unwrap_or("any");
                    let actual_type = match value {
                        Value::Null => "null",
                        Value::Bool(_) => "boolean",
                        Value::Number(_) => "number",
                        Value::String(_) => "string",
                        Value::Array(_) => "array",
                        Value::Object(_) => "object",
                    };
                    // Allow number to match integer type loosely
                    if expected_type != "any" && expected_type != actual_type 
                       && !(expected_type == "integer" && actual_type == "number") {
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
            Ok(output) => ToolResult::success(output),
            Err(e) => ToolResult::error(format!("MCP tool error: {}", e)),
        }
    }
}

/// Shared MCP connection manager
pub type SharedMcpManager = Arc<McpConnectionManager>;
use crate::business::mcp::{McpServerConfig, McpToolDef};
use crate::business::mcp_manager::{
    diff_tools, qualified_tool_name, removed_qualified_tool_names, ConnectionState,
    McpConnectionManager, McpManagerConfig, McpServerConnection,
};
use serde_json::json;
use std::collections::HashMap;

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
    assert_eq!(
        diff.changed[1].input_schema,
        schema_changed_new.input_schema
    );
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
        vec![
            "mcp__demo__read".to_string(),
            "mcp__demo__write".to_string()
        ]
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
        crate::business::mcp::DEFAULT_MAX_TOOL_RESPONSE_BYTES
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
        crate::business::mcp::DEFAULT_MAX_TOOL_RESPONSE_BYTES
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

#[tokio::test]
async fn test_mark_reconnecting_sets_state_and_clears_error() {
    let mut servers = HashMap::new();
    servers.insert(
        "example".to_string(),
        server_config(Some("/bin/example-mcp")),
    );
    let manager = McpConnectionManager::with_servers(servers);
    manager.initialize().await.unwrap();

    manager
        .set_failed("example", "connection refused".to_string())
        .await
        .unwrap();

    manager.mark_reconnecting("example").await.unwrap();

    let connection = manager.get_server("example").await.unwrap();
    assert_eq!(connection.state, ConnectionState::Reconnecting);
    assert!(connection.error.is_none());
}

#[tokio::test]
async fn test_mark_reconnecting_unknown_server_returns_error() {
    let manager = McpConnectionManager::new(McpManagerConfig::default());

    let err = manager.mark_reconnecting("missing").await.unwrap_err();

    assert_eq!(err, "Server 'missing' not found");
}

#[tokio::test]
async fn test_refresh_tool_snapshot_updates_discovered_tools() {
    let mut servers = HashMap::new();
    servers.insert(
        "example".to_string(),
        server_config(Some("/bin/example-mcp")),
    );
    let manager = McpConnectionManager::with_servers(servers);
    manager.initialize().await.unwrap();

    manager
        .refresh_tool_snapshot("example", vec![tool("old", "old description")])
        .await
        .unwrap();
    manager
        .refresh_tool_snapshot("example", vec![tool("new", "new description")])
        .await
        .unwrap();

    let all_tools = manager.get_all_tools().await;
    assert_eq!(all_tools.len(), 1);
    assert_eq!(all_tools[0].0, "example");
    assert_eq!(all_tools[0].1.name, "new");
    assert_eq!(all_tools[0].1.description, "new description");

    let connection = manager.get_server("example").await.unwrap();
    assert_eq!(connection.tools.len(), 1);
    assert_eq!(connection.tools[0].name, "new");
}

#[tokio::test]
async fn test_refresh_tool_snapshot_unknown_server_returns_error() {
    let manager = McpConnectionManager::new(McpManagerConfig::default());

    let err = manager
        .refresh_tool_snapshot("missing", vec![tool("phantom", "phantom description")])
        .await
        .unwrap_err();

    assert_eq!(err, "Server 'missing' not found");
    assert!(manager.get_all_tools().await.is_empty());
}

use crate::api::{Tool, ToolExecutionContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;

pub struct ConfigTool;

#[async_trait]
impl Tool for ConfigTool {
    fn name(&self) -> &str {
        "Config"
    }
    fn description(&self) -> &str {
        "View or modify configuration settings. Supports getting, setting, and listing config values."
    }
    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["get", "set", "list", "reset"],
                    "description": "Action to perform: get, set, list, or reset"
                },
                "key": {
                    "type": "string",
                    "description": "Configuration key (e.g., 'api_key', 'model', 'max_tokens')"
                },
                "value": {
                    "type": "string",
                    "description": "Value to set (only for 'set' action)"
                }
            },
            "required": ["action"]
        })
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(&self, input: Value, _ctx: &ToolExecutionContext) -> ToolResult {
        let action = input["action"].as_str().unwrap_or("list");
        let key = input["key"].as_str();
        let value = input["value"].as_str();

        match action {
            "list" => {
                // 列出所有可配置项
                let config_items = serde_json::json!([
                    { "key": "api_key", "description": "Anthropic API key (sensitive)" },
                    { "key": "model", "description": "Model to use (e.g., claude-sonnet-4-20250514)" },
                    { "key": "max_tokens", "description": "Maximum tokens per response" },
                    { "key": "temperature", "description": "Response randomness (0-1)" },
                    { "key": "system_prompt", "description": "Custom system prompt" },
                    { "key": "cwd", "description": "Working directory" },
                    { "key": "mcp_servers", "description": "MCP server configurations" },
                ]);

                ToolResult::success(serde_json::json!({
                    "status": "success",
                    "message": "Available configuration options listed",
                    "data": {
                        "options": config_items
                    }
                }).to_string())
            }
            "get" => {
                if key.is_none() {
                    return ToolResult::error(serde_json::json!({
                        "status": "error",
                        "message": "Key is required for 'get' action",
                        "data": {}
                    }).to_string());
                }
                let key = key.unwrap_or("unknown");
                ToolResult::success(serde_json::json!({
                    "status": "success",
                    "message": format!("Config '{}' retrieved", key),
                    "data": {
                        "key": key,
                        "hint": "Use environment variables or config file to set this value"
                    }
                }).to_string())
            }
            "set" => {
                let key = match key {
                    Some(k) => k,
                    None => return ToolResult::error(serde_json::json!({
                        "status": "error",
                        "message": "Key is required for 'set' action",
                        "data": {}
                    }).to_string()),
                };
                if value.is_none() {
                    return ToolResult::error(serde_json::json!({
                        "status": "error",
                        "message": "Value is required for 'set' action",
                        "data": {}
                    }).to_string());
                }

                ToolResult::success(serde_json::json!({
                    "status": "success",
                    "message": format!("To set config '{}', update your config file or set environment variable AEMEATH_{}", key, key.to_uppercase()),
                    "data": {
                        "key": key,
                        "env_var": format!("AEMEATH_{}", key.to_uppercase())
                    }
                }).to_string())
            }
            "reset" => {
                let key = match key {
                    Some(k) => k,
                    None => return ToolResult::error(serde_json::json!({
                        "status": "error",
                        "message": "Key is required for 'reset' action",
                        "data": {}
                    }).to_string()),
                };

                ToolResult::success(serde_json::json!({
                    "status": "success",
                    "message": format!("To reset config '{}', remove it from your config file or unset AEMEATH_{}", key, key.to_uppercase()),
                    "data": {
                        "key": key,
                        "env_var": format!("AEMEATH_{}", key.to_uppercase())
                    }
                }).to_string())
            }
            _ => ToolResult::error(serde_json::json!({
                "status": "error",
                "message": format!("Unknown action: {}", action),
                "data": {
                    "valid_actions": ["get", "set", "list", "reset"]
                }
            }).to_string()),
        }
    }
}

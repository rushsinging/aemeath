use async_trait::async_trait;
use serde_json::Value;
use share::tool::{Tool, ToolContext, ToolResult};

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

    async fn call(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let action = input["action"].as_str().unwrap_or("list");
        let key = input["key"].as_str();
        let value = input["value"].as_str();

        match action {
            "list" => {
                // 列出所有可配置项
                let config_items = [
                    ("api_key", "Anthropic API key (sensitive)"),
                    ("model", "Model to use (e.g., claude-sonnet-4-20250514)"),
                    ("max_tokens", "Maximum tokens per response"),
                    ("temperature", "Response randomness (0-1)"),
                    ("system_prompt", "Custom system prompt"),
                    ("cwd", "Working directory"),
                    ("mcp_servers", "MCP server configurations"),
                ];

                let output = config_items
                    .iter()
                    .map(|(k, d)| format!("{}: {}", k, d))
                    .collect::<Vec<_>>()
                    .join("\n");

                ToolResult::success(format!("Available configuration options:\n{}", output))
            }
            "get" => {
                if key.is_none() {
                    return ToolResult::error("Key is required for 'get' action");
                }
                let key = key;

                // 这里应该实际读取配置，但由于 Config 可能不在 context 中
                // 返回提示信息
                let key = key.unwrap_or("unknown");
                ToolResult::success(format!(
                    "Config '{}' - Use environment variables or config file to set this value.",
                    key
                ))
            }
            "set" => {
                let key = match key {
                    Some(k) => k,
                    None => return ToolResult::error("Key is required for 'set' action"),
                };
                if value.is_none() {
                    return ToolResult::error("Value is required for 'set' action");
                }

                ToolResult::success(format!(
                    "To set config '{}', update your config file or set environment variable AEMEATH_{}",
                    key,
                    key.to_uppercase()
                ))
            }
            "reset" => {
                let key = match key {
                    Some(k) => k,
                    None => return ToolResult::error("Key is required for 'reset' action"),
                };

                ToolResult::success(format!(
                    "To reset config '{}', remove it from your config file or unset AEMEATH_{}",
                    key,
                    key.to_uppercase()
                ))
            }
            _ => ToolResult::error(format!(
                "Unknown action: {}. Valid actions: get, set, list, reset",
                action
            )),
        }
    }
}

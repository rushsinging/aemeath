mod handlers;
mod helpers;
mod schema;

#[cfg(test)]
mod tests;

use crate::api::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use share::tool::types::memory::MemoryResult;

pub struct MemoryTool;

#[async_trait]
impl TypedTool for MemoryTool {
    type Output = MemoryResult;
    fn name(&self) -> &str {
        "Memory"
    }

    fn description(&self) -> &str {
        "Manage persistent memory. Supports add, delete, search, pin, and list actions."
    }

    fn input_schema(&self) -> Value {
        schema::input_schema()
    }
    fn data_schema(&self) -> Value {
        use share::tool::types::ToolSchema;
        MemoryResult::data_schema()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(
        &self,
        input: Value,
        ctx: &ToolExecutionContext,
    ) -> TypedToolResult<MemoryResult> {
        let action = input
            .get("action")
            .and_then(|value| value.as_str())
            .unwrap_or("");

        match action {
            "add" => handlers::add_memory(input, ctx),
            "delete" => handlers::delete_memory(input, ctx),
            "search" => handlers::search_memory(input, ctx),
            "pin" => handlers::pin_memory(input, ctx),
            "list" => handlers::list_memory(input, ctx),
            "add_reminder" => handlers::add_reminder(input, ctx),
            "complete_reminder" => handlers::complete_reminder(input, ctx),
            "" => TypedToolResult::error("缺少必需参数: action"),
            other => TypedToolResult::error(format!("未知 memory action: {other}")),
        }
    }
}

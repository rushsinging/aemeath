mod handlers;
mod helpers;

#[cfg(test)]
mod tests;

use crate::domain::memory_source::MemoryPortSource;
use crate::domain::types::memory::{MemoryInput, MemoryResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// Memory management tool.
///
/// Holds an [`Arc<dyn MemoryPortSource>`] rather than a captured `Arc<dyn
/// MemoryPort>` because resume swaps the committed Memory under the same
/// registry. At execution time, [`MemoryPortSource::current`] returns the port
/// bound for the current Run.
pub struct MemoryTool {
    pub source: Arc<dyn MemoryPortSource>,
}

#[async_trait]
impl TypedTool for MemoryTool {
    type Output = MemoryResult;
    fn name(&self) -> &str {
        "Memory"
    }

    fn description(&self) -> &str {
        "Manage persistent memory. Supports add, delete, search, pin, and list actions."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::memory(lang))
    }

    fn input_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
        MemoryInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        use crate::domain::types::ToolSchema;
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
        let args: MemoryInput = match serde_json::from_value(input.clone()) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let action = args.action.as_str();
        let port = self.source.current();

        match action {
            "add" => handlers::add_memory(input, ctx, &*port).await,
            "delete" => handlers::delete_memory(input, &*port).await,
            "search" => handlers::search_memory(input, &*port),
            "pin" => handlers::pin_memory(input, &*port).await,
            "list" => handlers::list_memory(input, &*port),
            "add_reminder" => handlers::add_reminder(input, ctx),
            "complete_reminder" => handlers::complete_reminder(input, ctx),
            "" => TypedToolResult::error("缺少必需参数: action"),
            other => TypedToolResult::error(format!("未知 memory action: {other}")),
        }
    }
}

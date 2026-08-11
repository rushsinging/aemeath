mod handlers;
mod helpers;

#[cfg(test)]
mod tests;
use crate::domain::memory_source::MemoryPortSource;
use crate::domain::types::memory::{MemoryAction, MemoryInput, MemoryResult};
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

fn memory_input_schema() -> Value {
    use crate::domain::types::ToolSchema;

    let mut schema = MemoryInput::data_schema();
    schema["oneOf"] = serde_json::json!([
        action_contract("add", &["action", "content"]),
        action_contract("delete", &["action", "id"]),
        action_contract("search", &["action", "query"]),
        action_contract("pin", &["action", "id"]),
        action_contract("list", &["action"]),
        action_contract("add_reminder", &["action", "content"]),
        action_contract("complete_reminder", &["action", "id"]),
    ]);
    schema
}

fn action_contract(action: &str, required: &[&str]) -> Value {
    serde_json::json!({
        "properties": {"action": {"const": action}},
        "required": required,
    })
}

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
        "Manage persistent Memory and current-session reminders. Supports typed add, delete, search, pin, list, add_reminder, and complete_reminder actions."
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::memory(lang))
    }

    fn input_schema(&self) -> Value {
        memory_input_schema()
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
        let port = self.source.current();
        match args.action {
            MemoryAction::Add => handlers::add_memory(input, ctx, &*port).await,
            MemoryAction::Delete => handlers::delete_memory(input, &*port).await,
            MemoryAction::Search => handlers::search_memory(input, &*port),
            MemoryAction::Pin => handlers::pin_memory(input, &*port).await,
            MemoryAction::List => handlers::list_memory(input, &*port),
            MemoryAction::AddReminder => handlers::add_reminder(input, ctx),
            MemoryAction::CompleteReminder => handlers::complete_reminder(input, ctx),
        }
    }
}

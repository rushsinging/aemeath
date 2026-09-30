mod handlers;
mod helpers;

#[cfg(test)]
mod tests;

use crate::domain::memory_source::MemoryPortSource;
use crate::domain::types::memory::{
    MemoryAddInput, MemoryDeleteInput, MemoryListInput, MemoryResult, MemorySearchInput,
    MemoryUpdateInput,
};
use crate::domain::types::ToolSchema;
use crate::domain::{ToolExecutionContext, TypedTool, TypedToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

/// Write one persistent memory entry.
///
/// Holds an [`Arc<dyn MemoryPortSource>`] rather than a captured `Arc<dyn
/// MemoryPort>` because resume swaps the committed Memory under the same
/// registry. At execution time, [`MemoryPortSource::current`] returns the port
/// bound for the current Run.
pub struct MemoryAddTool {
    pub source: Arc<dyn MemoryPortSource>,
}

/// Lexical search over persistent memory entries.
pub struct MemorySearchTool {
    pub source: Arc<dyn MemoryPortSource>,
}

/// List persistent memory entries.
pub struct MemoryListTool {
    pub source: Arc<dyn MemoryPortSource>,
}

/// Pin, unpin, archive, or restore one memory entry.
pub struct MemoryUpdateTool {
    pub source: Arc<dyn MemoryPortSource>,
}

/// Permanently delete one memory entry.
pub struct MemoryDeleteTool {
    pub source: Arc<dyn MemoryPortSource>,
}

#[async_trait]
impl TypedTool for MemoryAddTool {
    type Output = MemoryResult;

    fn name(&self) -> &str {
        "MemoryAdd"
    }

    fn description(&self) -> &str {
        share::i18n::tools::core::memory_add("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::memory_add(lang))
    }

    fn input_schema(&self) -> Value {
        MemoryAddInput::data_schema()
    }
    fn data_schema(&self) -> Value {
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
        let _args: MemoryAddInput = match serde_json::from_value(input.clone()) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let port = self.source.current();
        handlers::add_memory(input, ctx, &*port).await
    }
}

#[async_trait]
impl TypedTool for MemorySearchTool {
    type Output = MemoryResult;

    fn name(&self) -> &str {
        "MemorySearch"
    }

    fn description(&self) -> &str {
        share::i18n::tools::core::memory_search("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::memory_search(lang))
    }

    fn input_schema(&self) -> Value {
        MemorySearchInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        MemoryResult::data_schema()
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<MemoryResult> {
        let _args: MemorySearchInput = match serde_json::from_value(input.clone()) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let port = self.source.current();
        handlers::search_memory(input, &*port)
    }
}

#[async_trait]
impl TypedTool for MemoryListTool {
    type Output = MemoryResult;

    fn name(&self) -> &str {
        "MemoryList"
    }

    fn description(&self) -> &str {
        share::i18n::tools::core::memory_list("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::memory_list(lang))
    }

    fn input_schema(&self) -> Value {
        MemoryListInput::data_schema()
    }
    fn data_schema(&self) -> Value {
        MemoryResult::data_schema()
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<MemoryResult> {
        let _args: MemoryListInput = match serde_json::from_value(input.clone()) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let port = self.source.current();
        handlers::list_memory(input, &*port)
    }
}

#[async_trait]
impl TypedTool for MemoryUpdateTool {
    type Output = MemoryResult;

    fn name(&self) -> &str {
        "MemoryUpdate"
    }

    fn description(&self) -> &str {
        share::i18n::tools::core::memory_update("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::memory_update(lang))
    }

    fn input_schema(&self) -> Value {
        MemoryUpdateInput::data_schema()
    }
    fn data_schema(&self) -> Value {
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
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<MemoryResult> {
        let args: MemoryUpdateInput = match serde_json::from_value(input.clone()) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let port = self.source.current();
        handlers::update_memory(&args.id, args.status, &*port).await
    }
}

#[async_trait]
impl TypedTool for MemoryDeleteTool {
    type Output = MemoryResult;

    fn name(&self) -> &str {
        "MemoryDelete"
    }

    fn description(&self) -> &str {
        share::i18n::tools::core::memory_delete("en")
    }
    fn description_for(&self, lang: &str) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(share::i18n::tools::core::memory_delete(lang))
    }

    fn input_schema(&self) -> Value {
        MemoryDeleteInput::data_schema()
    }
    fn data_schema(&self) -> Value {
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
        _ctx: &ToolExecutionContext,
    ) -> TypedToolResult<MemoryResult> {
        let _args: MemoryDeleteInput = match serde_json::from_value(input.clone()) {
            Ok(a) => a,
            Err(e) => return TypedToolResult::error(format!("invalid input: {e}")),
        };
        let port = self.source.current();
        handlers::delete_memory(input, &*port).await
    }
}

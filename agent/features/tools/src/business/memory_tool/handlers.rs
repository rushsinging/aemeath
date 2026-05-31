use crate::api::{ToolContext, ToolResult};
use serde_json::Value;
use share::memory_ops::{
    format_add_result, format_memory_list, MemoryCategory, MemoryEntry, MemoryLayer, MemorySource,
};

use super::helpers::{
    open_store, optional_category, optional_layer, parse_tags, required_string, validate_content,
};

pub(super) fn add_memory(input: Value, ctx: &ToolContext) -> ToolResult {
    let content = match input.get("content").and_then(|value| value.as_str()) {
        Some(content) => content.trim(),
        None => return ToolResult::error("缺少必需参数: content"),
    };
    if let Err(error) = validate_content(content) {
        return ToolResult::error(error);
    }

    let layer = match optional_layer(&input) {
        Ok(layer) => layer.unwrap_or(MemoryLayer::Project),
        Err(error) => return ToolResult::error(error),
    };
    let category = match optional_category(&input) {
        Ok(category) => category.unwrap_or(MemoryCategory::Fact),
        Err(error) => return ToolResult::error(error),
    };
    let tags = match parse_tags(&input) {
        Ok(tags) => tags,
        Err(error) => return ToolResult::error(error),
    };

    let mut entry = MemoryEntry::new(layer, category, content, MemorySource::Llm).with_tags(tags);
    entry.pinned = input
        .get("pinned")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if let Some(session_id) = &ctx.parent_session_id {
        entry.source_ref = Some(session_id.clone());
    }

    let mut store = match open_store(ctx) {
        Ok(store) => store,
        Err(error) => return ToolResult::error(error),
    };
    match store.add(entry) {
        Ok(result) => ToolResult::success(format_add_result(result)),
        Err(error) => ToolResult::error(error.to_string()),
    }
}

pub(super) fn delete_memory(input: Value, ctx: &ToolContext) -> ToolResult {
    let id = match required_string(&input, "id") {
        Ok(id) => id,
        Err(error) => return ToolResult::error(error),
    };
    let mut store = match open_store(ctx) {
        Ok(store) => store,
        Err(error) => return ToolResult::error(error),
    };

    match store.delete(id) {
        Ok(()) => ToolResult::success("记忆已删除。"),
        Err(error) => ToolResult::error(error.to_string()),
    }
}

pub(super) fn search_memory(input: Value, ctx: &ToolContext) -> ToolResult {
    let query = match required_string(&input, "query") {
        Ok(query) => query,
        Err(error) => return ToolResult::error(error),
    };
    let limit = input
        .get("limit")
        .and_then(|value| value.as_u64())
        .unwrap_or(10) as usize;
    let store = match open_store(ctx) {
        Ok(store) => store,
        Err(error) => return ToolResult::error(error),
    };

    match store.search(query, limit.min(50)) {
        Ok(entries) => ToolResult::success(format_memory_list(&entries)),
        Err(error) => ToolResult::error(error.to_string()),
    }
}

pub(super) fn pin_memory(input: Value, ctx: &ToolContext) -> ToolResult {
    let id = match required_string(&input, "id") {
        Ok(id) => id,
        Err(error) => return ToolResult::error(error),
    };
    let pinned = input
        .get("pinned")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let mut store = match open_store(ctx) {
        Ok(store) => store,
        Err(error) => return ToolResult::error(error),
    };

    match store.pin(id, pinned) {
        Ok(()) if pinned => ToolResult::success("记忆已固定。"),
        Ok(()) => ToolResult::success("记忆已取消固定。"),
        Err(error) => ToolResult::error(error.to_string()),
    }
}

pub(super) fn list_memory(input: Value, ctx: &ToolContext) -> ToolResult {
    let layer = match optional_layer(&input) {
        Ok(layer) => layer,
        Err(error) => return ToolResult::error(error),
    };
    let store = match open_store(ctx) {
        Ok(store) => store,
        Err(error) => return ToolResult::error(error),
    };

    match store.list(layer) {
        Ok(entries) => ToolResult::success(format_memory_list(&entries)),
        Err(error) => ToolResult::error(error.to_string()),
    }
}

pub(super) fn add_reminder(input: Value, ctx: &ToolContext) -> ToolResult {
    let content = match required_string(&input, "content") {
        Ok(content) => content,
        Err(error) => return ToolResult::error(error),
    };
    if let Err(error) = validate_content(content) {
        return ToolResult::error(error);
    }
    let priority = input
        .get("priority")
        .and_then(|value| value.as_str())
        .unwrap_or("normal");
    if !matches!(priority, "low" | "normal" | "high") {
        return ToolResult::error(format!("无效 reminder priority: {priority}"));
    }

    let Some(reminders) = &ctx.session_reminders else {
        return ToolResult::error("当前运行环境不支持 session reminder。");
    };
    match reminders.lock() {
        Ok(mut reminders) => match reminders.add(content.to_string()) {
            Ok(id) => ToolResult::success(format!("已添加会话提醒: {id}")),
            Err(error) => ToolResult::error(error.to_string()),
        },
        Err(_) => ToolResult::error("session reminder 状态锁已损坏"),
    }
}

pub(super) fn complete_reminder(input: Value, ctx: &ToolContext) -> ToolResult {
    let id = match required_string(&input, "id") {
        Ok(id) => id,
        Err(error) => return ToolResult::error(error),
    };
    let Some(reminders) = &ctx.session_reminders else {
        return ToolResult::error("当前运行环境不支持 session reminder。");
    };
    match reminders.lock() {
        Ok(mut reminders) => match reminders.complete(id) {
            Ok(()) => ToolResult::success("会话提醒已完成。"),
            Err(error) => ToolResult::error(error.to_string()),
        },
        Err(_) => ToolResult::error("session reminder 状态锁已损坏"),
    }
}

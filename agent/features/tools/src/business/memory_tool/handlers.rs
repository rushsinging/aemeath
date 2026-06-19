use crate::api::{ToolExecutionContext, ToolResult};
use serde_json::Value;
use share::memory_ops::{AddResult, MemoryCategory, MemoryEntry, MemoryLayer, MemorySource};
use share::tool::types::memory::MemoryResult;
use std::time::{SystemTime, UNIX_EPOCH};

use super::helpers::{
    open_store, optional_category, optional_layer, parse_tags, required_string, validate_content,
};

pub(super) fn add_memory(input: Value, ctx: &ToolExecutionContext) -> ToolResult {
    let content = match input.get("content").and_then(|value| value.as_str()) {
        Some(content) => content.trim(),
        None => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": "缺少必需参数: content",
                "data": {}
            }))
        }
    };
    if let Err(error) = validate_content(content) {
        return ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": error,
            "data": {}
        }));
    }

    let layer = match optional_layer(&input) {
        Ok(layer) => layer.unwrap_or(MemoryLayer::Project),
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };
    let category = match optional_category(&input) {
        Ok(category) => category.unwrap_or(MemoryCategory::Fact),
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };
    let tags = match parse_tags(&input) {
        Ok(tags) => tags,
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };

    let now = current_timestamp_secs();
    let mut entry = MemoryEntry::new(
        uuid::Uuid::now_v7().to_string(),
        now,
        layer,
        category,
        content,
        MemorySource::Llm,
    )
    .with_tags(tags);
    entry.pinned = input
        .get("pinned")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if let Some(session_id) = &ctx.parent_session_id {
        entry.source_ref = Some(session_id.clone());
    }

    let mut store = match open_store(ctx) {
        Ok(store) => store,
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };
    match store.add(entry) {
        Ok(AddResult::Added { id }) => ToolResult::success_json(serde_json::json!({
            "status": "success",
            "message": format!("记忆已添加。ID: {}", &id[..8.min(id.len())]),
            "data": serde_json::to_value(MemoryResult { action: "added".to_string() }).unwrap()
        })),
        Ok(AddResult::Merged { existing_id }) => ToolResult::success_json(serde_json::json!({
            "status": "success",
            "message": format!("已与相似记忆合并: {}", &existing_id[..8.min(existing_id.len())]),
            "data": serde_json::to_value(MemoryResult { action: "merged".to_string() }).unwrap()
        })),
        Ok(AddResult::NeedsEviction { candidates }) => ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": "记忆数量已达上限，请先归档候选记忆",
            "data": { "candidates": candidates }
        })),
        Err(error) => ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "data": {}
        })),
    }
}

pub(super) fn delete_memory(input: Value, ctx: &ToolExecutionContext) -> ToolResult {
    let id = match required_string(&input, "id") {
        Ok(id) => id,
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };
    let mut store = match open_store(ctx) {
        Ok(store) => store,
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };

    match store.delete(id) {
        Ok(()) => ToolResult::success_json(serde_json::json!({
            "status": "success",
            "message": "记忆已删除。",
            "data": serde_json::to_value(MemoryResult { action: "delete".to_string() }).unwrap()
        })),
        Err(error) => ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "data": { "id": id }
        })),
    }
}

pub(super) fn search_memory(input: Value, ctx: &ToolExecutionContext) -> ToolResult {
    let query = match required_string(&input, "query") {
        Ok(query) => query,
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };
    let limit = input
        .get("limit")
        .and_then(|value| value.as_u64())
        .unwrap_or(10) as usize;
    let store = match open_store(ctx) {
        Ok(store) => store,
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };

    match store.search(query, limit.min(50)) {
        Ok(entries) => {
            let message = if entries.is_empty() {
                "暂无记忆。".to_string()
            } else {
                format!("找到 {} 条记忆。", entries.len())
            };
            ToolResult::success_json(serde_json::json!({
                "status": "success",
                "message": message,
                "data": serde_json::to_value(MemoryResult { action: "search".to_string() }).unwrap()
            }))
        }
        Err(error) => ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "data": {}
        })),
    }
}

pub(super) fn pin_memory(input: Value, ctx: &ToolExecutionContext) -> ToolResult {
    let id = match required_string(&input, "id") {
        Ok(id) => id,
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };
    let pinned = input
        .get("pinned")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let mut store = match open_store(ctx) {
        Ok(store) => store,
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };

    match store.pin(id, pinned) {
        Ok(()) => ToolResult::success_json(serde_json::json!({
            "status": "success",
            "message": if pinned { "记忆已固定。" } else { "记忆已取消固定。" },
            "data": serde_json::to_value(MemoryResult { action: "pin".to_string() }).unwrap()
        })),
        Err(error) => ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "data": { "id": id }
        })),
    }
}

pub(super) fn list_memory(input: Value, ctx: &ToolExecutionContext) -> ToolResult {
    let layer = match optional_layer(&input) {
        Ok(layer) => layer,
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };
    let store = match open_store(ctx) {
        Ok(store) => store,
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };

    match store.list(layer) {
        Ok(entries) => {
            let message = if entries.is_empty() {
                "暂无记忆。".to_string()
            } else {
                format!("共 {} 条记忆。", entries.len())
            };
            ToolResult::success_json(serde_json::json!({
                "status": "success",
                "message": message,
                "data": serde_json::to_value(MemoryResult { action: "list".to_string() }).unwrap()
            }))
        }
        Err(error) => ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": error.to_string(),
            "data": {}
        })),
    }
}

fn current_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(super) fn add_reminder(input: Value, ctx: &ToolExecutionContext) -> ToolResult {
    let content = match required_string(&input, "content") {
        Ok(content) => content,
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };
    if let Err(error) = validate_content(content) {
        return ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": error,
            "data": {}
        }));
    }
    let priority = input
        .get("priority")
        .and_then(|value| value.as_str())
        .unwrap_or("normal");
    if !matches!(priority, "low" | "normal" | "high") {
        return ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": format!("无效 reminder priority: {priority}"),
            "data": { "priority": priority }
        }));
    }

    let Some(reminders) = &ctx.session_reminders else {
        return ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": "当前运行环境不支持 session reminder。",
            "data": {}
        }));
    };
    match reminders.lock() {
        Ok(mut reminders) => {
            let id = uuid::Uuid::now_v7().to_string();
            match reminders.add(id.clone(), content.to_string(), current_timestamp_secs()) {
                Ok(id) => ToolResult::success_json(serde_json::json!({
                    "status": "success",
                    "message": format!("已添加会话提醒: {id}"),
                    "data": serde_json::to_value(MemoryResult { action: "add_reminder".to_string() }).unwrap()
                })),
                Err(error) => ToolResult::error_json(serde_json::json!({
                    "status": "error",
                    "message": error.to_string(),
                    "data": {}
                })),
            }
        }
        Err(_) => ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": "session reminder 状态锁已损坏",
            "data": {}
        })),
    }
}

pub(super) fn complete_reminder(input: Value, ctx: &ToolExecutionContext) -> ToolResult {
    let id = match required_string(&input, "id") {
        Ok(id) => id,
        Err(error) => {
            return ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error,
                "data": {}
            }))
        }
    };
    let Some(reminders) = &ctx.session_reminders else {
        return ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": "当前运行环境不支持 session reminder。",
            "data": {}
        }));
    };
    match reminders.lock() {
        Ok(mut reminders) => match reminders.complete(id) {
            Ok(()) => ToolResult::success_json(serde_json::json!({
                "status": "success",
                "message": "会话提醒已完成。",
                "data": serde_json::to_value(MemoryResult { action: "complete_reminder".to_string() }).unwrap()
            })),
            Err(error) => ToolResult::error_json(serde_json::json!({
                "status": "error",
                "message": error.to_string(),
                "data": { "id": id }
            })),
        },
        Err(_) => ToolResult::error_json(serde_json::json!({
            "status": "error",
            "message": "session reminder 状态锁已损坏",
            "data": {}
        })),
    }
}

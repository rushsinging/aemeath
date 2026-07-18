use crate::domain::types::memory::MemoryResult;
use crate::domain::{ToolExecutionContext, TypedToolResult};
use memory::api::{
    MemoryCategory as Category, MemoryEntry, MemoryId as Id, MemoryLayer as Layer,
    MemorySearchQuery as Query, MemorySource as Source, WriteResult,
};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use super::helpers::{
    optional_category, optional_layer, parse_tags, required_string, validate_content,
};

pub(super) async fn add_memory(
    input: Value,
    ctx: &ToolExecutionContext,
) -> TypedToolResult<MemoryResult> {
    let content = match input.get("content").and_then(|value| value.as_str()) {
        Some(content) => content.trim(),
        None => return TypedToolResult::error("缺少必需参数: content"),
    };
    if let Err(error) = validate_content(content) {
        return TypedToolResult::error(error);
    }

    let layer = match optional_layer(&input) {
        Ok(layer) => layer.unwrap_or(Layer::Project),
        Err(error) => return TypedToolResult::error(error),
    };
    let category = match optional_category(&input) {
        Ok(category) => category.unwrap_or(Category::Fact),
        Err(error) => return TypedToolResult::error(error),
    };
    let tags = match parse_tags(&input) {
        Ok(tags) => tags,
        Err(error) => return TypedToolResult::error(error),
    };

    let now = current_timestamp_secs();
    let mut entry = match MemoryEntry::new(Id::now_v7(), now, layer, category, content, Source::Llm)
    {
        Ok(entry) => entry,
        Err(error) => return TypedToolResult::error(error.to_string()),
    };
    entry.tags = tags;
    entry.pinned = input
        .get("pinned")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if let Some(session_id) = ctx.parent_session_id() {
        entry.source_ref = Some(session_id);
    }

    match ctx.memory().write(entry).await {
        Ok(WriteResult::Added { id }) => {
            let id = id.to_string();
            TypedToolResult::success(
                format!("记忆已添加。ID: {}", &id[..8.min(id.len())]),
                MemoryResult {
                    action: "added".to_string(),
                },
            )
        }
        Ok(WriteResult::Merged { existing_id }) => {
            let existing_id = existing_id.to_string();
            TypedToolResult::success(
                format!(
                    "已与相似记忆合并: {}",
                    &existing_id[..8.min(existing_id.len())]
                ),
                MemoryResult {
                    action: "merged".to_string(),
                },
            )
        }
        Ok(WriteResult::NeedsEviction { candidates: _ }) => {
            TypedToolResult::error("记忆数量已达上限，请先归档候选记忆")
        }
        Ok(WriteResult::NoOp) => TypedToolResult::success(
            "记忆写入已跳过。",
            MemoryResult {
                action: "noop".to_string(),
            },
        ),
        Err(error) => TypedToolResult::error(error.to_string()),
    }
}

pub(super) async fn delete_memory(
    input: Value,
    ctx: &ToolExecutionContext,
) -> TypedToolResult<MemoryResult> {
    let id =
        match required_string(&input, "id").and_then(|id| Id::new(id).map_err(|e| e.to_string())) {
            Ok(id) => id,
            Err(error) => return TypedToolResult::error(error),
        };

    match ctx.memory().delete(&id).await {
        Ok(true) => TypedToolResult::success(
            "记忆已删除。",
            MemoryResult {
                action: "delete".to_string(),
            },
        ),
        Ok(false) => TypedToolResult::error("记忆不存在。"),
        Err(error) => TypedToolResult::error(error.to_string()),
    }
}

pub(super) async fn search_memory(
    input: Value,
    ctx: &ToolExecutionContext,
) -> TypedToolResult<MemoryResult> {
    let text = match required_string(&input, "query") {
        Ok(query) => query.to_string(),
        Err(error) => return TypedToolResult::error(error),
    };
    let limit = input
        .get("limit")
        .and_then(|value| value.as_u64())
        .unwrap_or(10) as usize;
    let query = Query {
        text,
        limit: limit.min(50),
        layer: None,
        category: None,
        include_archive: false,
        now: current_timestamp_secs(),
    };
    let result = ctx.memory().search(&query);
    let message = if result.hits.is_empty() {
        "暂无记忆。".to_string()
    } else {
        format!("找到 {} 条记忆。", result.hits.len())
    };
    TypedToolResult::success(
        message,
        MemoryResult {
            action: "search".to_string(),
        },
    )
}

pub(super) async fn pin_memory(
    input: Value,
    ctx: &ToolExecutionContext,
) -> TypedToolResult<MemoryResult> {
    let id =
        match required_string(&input, "id").and_then(|id| Id::new(id).map_err(|e| e.to_string())) {
            Ok(id) => id,
            Err(error) => return TypedToolResult::error(error),
        };
    let pinned = input
        .get("pinned")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);

    match ctx.memory().pin(&id, pinned).await {
        Ok(true) => TypedToolResult::success(
            if pinned {
                "记忆已固定。"
            } else {
                "记忆已取消固定。"
            },
            MemoryResult {
                action: "pin".to_string(),
            },
        ),
        Ok(false) => TypedToolResult::error("记忆不存在。"),
        Err(error) => TypedToolResult::error(error.to_string()),
    }
}

pub(super) async fn list_memory(
    input: Value,
    ctx: &ToolExecutionContext,
) -> TypedToolResult<MemoryResult> {
    let layer = match optional_layer(&input) {
        Ok(layer) => layer,
        Err(error) => return TypedToolResult::error(error),
    };
    let entries = ctx.memory().list(layer);
    let message = if entries.is_empty() {
        "暂无记忆。".to_string()
    } else {
        format!("共 {} 条记忆。", entries.len())
    };
    TypedToolResult::success(
        message,
        MemoryResult {
            action: "list".to_string(),
        },
    )
}

fn current_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(super) fn add_reminder(
    input: Value,
    ctx: &ToolExecutionContext,
) -> TypedToolResult<MemoryResult> {
    let content = match required_string(&input, "content") {
        Ok(content) => content,
        Err(error) => return TypedToolResult::error(error),
    };
    if let Err(error) = validate_content(content) {
        return TypedToolResult::error(error);
    }
    let priority = input
        .get("priority")
        .and_then(|value| value.as_str())
        .unwrap_or("normal");
    if !matches!(priority, "low" | "normal" | "high") {
        return TypedToolResult::error(format!("无效 reminder priority: {priority}"));
    }

    let Some(reminders) = ctx.session_reminders() else {
        return TypedToolResult::error("当前运行环境不支持 session reminder。");
    };
    let result = match reminders.lock() {
        Ok(mut reminders) => {
            let id = uuid::Uuid::now_v7().to_string();
            match reminders.add(id.clone(), content.to_string(), current_timestamp_secs()) {
                Ok(id) => TypedToolResult::success(
                    format!("已添加会话提醒: {id}"),
                    MemoryResult {
                        action: "add_reminder".to_string(),
                    },
                ),
                Err(error) => TypedToolResult::error(error.to_string()),
            }
        }
        Err(_) => TypedToolResult::error("session reminder 状态锁已损坏"),
    };
    result
}

pub(super) fn complete_reminder(
    input: Value,
    ctx: &ToolExecutionContext,
) -> TypedToolResult<MemoryResult> {
    let id = match required_string(&input, "id") {
        Ok(id) => id,
        Err(error) => return TypedToolResult::error(error),
    };
    let Some(reminders) = ctx.session_reminders() else {
        return TypedToolResult::error("当前运行环境不支持 session reminder。");
    };
    let result = match reminders.lock() {
        Ok(mut reminders) => match reminders.complete(id) {
            Ok(()) => TypedToolResult::success(
                "会话提醒已完成。",
                MemoryResult {
                    action: "complete_reminder".to_string(),
                },
            ),
            Err(error) => TypedToolResult::error(error.to_string()),
        },
        Err(_) => TypedToolResult::error("session reminder 状态锁已损坏"),
    };
    result
}

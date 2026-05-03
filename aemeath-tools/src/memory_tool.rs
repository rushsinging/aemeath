use aemeath_core::memory::{
    format_add_result, format_memory_list, memory_base_dir, parse_category, parse_layer,
    project_hash_from_path, MemoryCategory, MemoryEntry, MemoryLayer, MemorySource, MemoryStore,
};
use aemeath_core::tool::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;

const MAX_CONTENT_CHARS: usize = 500;
const MAX_TAGS: usize = 10;
const MAX_TAG_CHARS: usize = 32;

pub struct MemoryTool;

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "Memory"
    }

    fn description(&self) -> &str {
        "Manage persistent memory. Supports add, delete, search, pin, and list actions."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "delete", "search", "pin", "list", "add_reminder", "complete_reminder"],
                    "description": "Memory action to perform"
                },
                "id": { "type": "string", "description": "Memory id for delete/pin actions" },
                "content": { "type": "string", "description": "Memory content, max 500 chars" },
                "query": { "type": "string", "description": "Search query" },
                "limit": { "type": "integer", "description": "Maximum number of results" },
                "layer": {
                    "type": "string",
                    "enum": ["global", "project"],
                    "description": "Memory layer"
                },
                "category": {
                    "type": "string",
                    "enum": ["fact", "decision", "preference", "pattern", "pitfall"],
                    "description": "Memory category"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional tags"
                },
                "pinned": { "type": "boolean", "description": "Whether to pin the memory" },
              "priority": {
                  "type": "string",
                  "enum": ["low", "normal", "high"],
                  "description": "Reminder priority"
              }
            },
            "required": ["action"]
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let action = input
            .get("action")
            .and_then(|value| value.as_str())
            .unwrap_or("");

        match action {
            "add" => add_memory(input, ctx),
            "delete" => delete_memory(input, ctx),
            "search" => search_memory(input, ctx),
            "pin" => pin_memory(input, ctx),
            "list" => list_memory(input, ctx),
            "add_reminder" => add_reminder(input, ctx),
            "complete_reminder" => complete_reminder(input, ctx),
            "" => ToolResult::error("缺少必需参数: action"),
            other => ToolResult::error(format!("未知 memory action: {other}")),
        }
    }
}

fn add_memory(input: Value, ctx: &ToolContext) -> ToolResult {
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

fn delete_memory(input: Value, ctx: &ToolContext) -> ToolResult {
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

fn search_memory(input: Value, ctx: &ToolContext) -> ToolResult {
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

fn pin_memory(input: Value, ctx: &ToolContext) -> ToolResult {
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

fn list_memory(input: Value, ctx: &ToolContext) -> ToolResult {
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

fn add_reminder(input: Value, ctx: &ToolContext) -> ToolResult {
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

fn complete_reminder(input: Value, ctx: &ToolContext) -> ToolResult {
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

fn open_store(ctx: &ToolContext) -> Result<MemoryStore, String> {
    open_store_with_base(ctx, memory_base_dir())
}

fn open_store_with_base(ctx: &ToolContext, base_dir: PathBuf) -> Result<MemoryStore, String> {
    MemoryStore::new(base_dir, project_hash_from_path(&ctx.cwd), 100, 0.8)
        .map_err(|error| error.to_string())
}

fn required_string<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("缺少必需参数: {key}"))
}

fn optional_layer(input: &Value) -> Result<Option<MemoryLayer>, String> {
    match input.get("layer").and_then(|value| value.as_str()) {
        Some(layer) => parse_layer(layer)
            .map(Some)
            .ok_or_else(|| format!("无效 memory layer: {layer}")),
        None => Ok(None),
    }
}

fn optional_category(input: &Value) -> Result<Option<MemoryCategory>, String> {
    match input.get("category").and_then(|value| value.as_str()) {
        Some(category) => parse_category(category)
            .map(Some)
            .ok_or_else(|| format!("无效 memory category: {category}")),
        None => Ok(None),
    }
}

fn parse_tags(input: &Value) -> Result<Vec<String>, String> {
    let Some(tags) = input.get("tags").and_then(|value| value.as_array()) else {
        return Ok(Vec::new());
    };
    if tags.len() > MAX_TAGS {
        return Err(format!("tags 不能超过 {MAX_TAGS} 个"));
    }

    let mut parsed = Vec::new();
    for tag in tags {
        let Some(tag) = tag.as_str() else {
            return Err("tag 必须是字符串".to_string());
        };
        let tag = tag.trim();
        if tag.is_empty() {
            return Err("tag 不能为空".to_string());
        }
        if tag.chars().count() > MAX_TAG_CHARS {
            return Err(format!("tag 不能超过 {MAX_TAG_CHARS} 字符"));
        }
        parsed.push(tag.to_string());
    }
    parsed.sort();
    parsed.dedup();
    Ok(parsed)
}

fn validate_content(content: &str) -> Result<(), String> {
    if content.trim().is_empty() {
        return Err("memory content 不能为空".to_string());
    }
    if content.chars().count() > MAX_CONTENT_CHARS {
        return Err(format!("memory content 不能超过 {MAX_CONTENT_CHARS} 字符"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    fn test_ctx(cwd: PathBuf) -> ToolContext {
        ToolContext {
            cwd,
            cancel: CancellationToken::new(),
            read_files: Arc::new(Mutex::new(HashSet::new())),
            agent_runner: None,
            session_reminders: Some(Arc::new(Mutex::new(
                aemeath_core::memory::SessionReminders::new(),
            ))),
            plan_mode: None,
            allow_all: false,
            max_tool_concurrency: 10,
            max_agent_concurrency: 4,
            agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            progress_tx: None,
            parent_session_id: Some("test-session".to_string()),
        }
    }

    #[test]
    fn test_validate_content_normal() {
        assert!(validate_content("记住这个决策").is_ok());
    }

    #[test]
    fn test_validate_content_empty() {
        assert!(validate_content("   ").is_err());
    }

    #[test]
    fn test_validate_content_too_long() {
        let content = "x".repeat(MAX_CONTENT_CHARS + 1);
        assert!(validate_content(&content).is_err());
    }

    #[test]
    fn test_parse_tags_normal() {
        let input = serde_json::json!({"tags": ["rust", "rust", " memory "]});
        let tags = parse_tags(&input).unwrap();

        assert_eq!(tags, vec!["memory", "rust"]);
    }

    #[test]
    fn test_parse_tags_empty_array() {
        let input = serde_json::json!({"tags": []});
        let tags = parse_tags(&input).unwrap();

        assert!(tags.is_empty());
    }

    #[test]
    fn test_parse_tags_invalid_item() {
        let input = serde_json::json!({"tags": [1]});

        assert!(parse_tags(&input).is_err());
    }

    #[tokio::test]
    async fn test_memory_tool_add_and_search() {
        let dir = tempdir().unwrap();
        let ctx = test_ctx(dir.path().join("project"));
        let mut store = open_store_with_base(&ctx, dir.path().to_path_buf()).unwrap();
        let entry = MemoryEntry::new(
            MemoryLayer::Project,
            MemoryCategory::Decision,
            "使用 MemoryTool 管理记忆",
            MemorySource::Llm,
        );
        store.add(entry).unwrap();

        let results = store.search("MemoryTool", 5).unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("MemoryTool"));
    }
}

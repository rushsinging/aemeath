use crate::domain::types::memory::{
    MemoryCategoryInput, MemoryEntryResult, MemoryEvictionCandidateResult, MemoryLayerInput,
    MemoryLocationResult, MemoryResult, MemorySearchHitResult,
};
use crate::domain::{ToolExecutionContext, TypedToolResult};
use memory::api::{
    EvictionCandidate, MemoryCategory as Category, MemoryEntry, MemoryId as Id,
    MemoryLayer as Layer, MemoryLocation, MemoryPort, MemorySearchHit, MemorySearchQuery as Query,
    MemorySource as Source, RestoreResult, WriteResult,
};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use super::helpers::{
    optional_category, optional_layer, parse_tags, required_string, validate_content,
};

pub(super) async fn add_memory(
    input: Value,
    ctx: &ToolExecutionContext,
    port: &dyn MemoryPort,
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

    match port.write(entry).await {
        Ok(WriteResult::Added { id }) => TypedToolResult::success(
            format!("记忆已添加。ID: {id}"),
            MemoryResult {
                action: "added".to_string(),
                id: Some(id.to_string()),
                ..MemoryResult::default()
            },
        ),
        Ok(WriteResult::Merged { existing_id }) => TypedToolResult::success(
            format!("已与相似记忆合并: {existing_id}"),
            MemoryResult {
                action: "merged".to_string(),
                id: Some(existing_id.to_string()),
                ..MemoryResult::default()
            },
        ),
        Ok(WriteResult::NeedsEviction { candidates }) => {
            let candidate_results = candidates
                .iter()
                .map(eviction_candidate_result)
                .collect::<Vec<_>>();
            TypedToolResult::success(
                render_eviction_candidates(&candidates),
                MemoryResult {
                    action: "needs_eviction".to_string(),
                    eviction_candidates: Some(candidate_results),
                    ..MemoryResult::default()
                },
            )
        }
        Ok(WriteResult::NoOp) => TypedToolResult::success(
            "记忆写入已跳过。",
            MemoryResult {
                action: "noop".to_string(),
                ..MemoryResult::default()
            },
        ),
        Err(error) => TypedToolResult::error(error.to_string()),
    }
}

pub(super) async fn delete_memory(
    input: Value,
    port: &dyn MemoryPort,
) -> TypedToolResult<MemoryResult> {
    let id =
        match required_string(&input, "id").and_then(|id| Id::new(id).map_err(|e| e.to_string())) {
            Ok(id) => id,
            Err(error) => return TypedToolResult::error(error),
        };

    match port.delete(&id).await {
        Ok(true) => TypedToolResult::success(
            "记忆已删除。",
            MemoryResult {
                action: "delete".to_string(),
                id: Some(id.to_string()),
                ..MemoryResult::default()
            },
        ),
        Ok(false) => TypedToolResult::error("记忆不存在。"),
        Err(error) => TypedToolResult::error(error.to_string()),
    }
}

pub(super) fn search_memory(input: Value, port: &dyn MemoryPort) -> TypedToolResult<MemoryResult> {
    let text = match required_string(&input, "query") {
        Ok(query) => query.to_string(),
        Err(error) => return TypedToolResult::error(error),
    };
    let limit = input
        .get("limit")
        .and_then(|value| value.as_u64())
        .unwrap_or(10) as usize;
    let layer = match optional_layer(&input) {
        Ok(layer) => layer,
        Err(error) => return TypedToolResult::error(error),
    };
    let category = match optional_category(&input) {
        Ok(category) => category,
        Err(error) => return TypedToolResult::error(error),
    };
    let now = current_timestamp_secs();
    let query = Query {
        text,
        limit: limit.min(50),
        layer,
        category,
        include_archive: false,
        now,
    };
    let result = port.search(&query);
    let message = render_search_hits(&result.hits);
    TypedToolResult::success(
        message,
        MemoryResult {
            action: "search".to_string(),
            hits: Some(result.hits.iter().map(search_hit_result).collect()),
            ..MemoryResult::default()
        },
    )
}

pub(super) async fn pin_memory(
    input: Value,
    port: &dyn MemoryPort,
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

    match port.pin(&id, pinned).await {
        Ok(true) => TypedToolResult::success(
            if pinned {
                "记忆已固定。"
            } else {
                "记忆已取消固定。"
            },
            MemoryResult {
                action: "pin".to_string(),
                id: Some(id.to_string()),
                ..MemoryResult::default()
            },
        ),
        Ok(false) => TypedToolResult::error("记忆不存在。"),
        Err(error) => TypedToolResult::error(error.to_string()),
    }
}

pub(super) fn list_memory(input: Value, port: &dyn MemoryPort) -> TypedToolResult<MemoryResult> {
    let layer = match optional_layer(&input) {
        Ok(layer) => layer,
        Err(error) => return TypedToolResult::error(error),
    };
    let entries = port.list(layer);
    let message = render_memory_entries(&entries, current_timestamp_secs());
    TypedToolResult::success(
        message,
        MemoryResult {
            action: "list".to_string(),
            entries: Some(
                entries
                    .iter()
                    .map(|entry| memory_entry_result(entry, current_timestamp_secs()))
                    .collect(),
            ),
            ..MemoryResult::default()
        },
    )
}

pub(super) async fn archive_memory(
    input: Value,
    port: &dyn MemoryPort,
) -> TypedToolResult<MemoryResult> {
    let id = match required_string(&input, "id")
        .and_then(|id| Id::new(id).map_err(|error| error.to_string()))
    {
        Ok(id) => id,
        Err(error) => return TypedToolResult::error(error),
    };
    match port.archive(std::slice::from_ref(&id)).await {
        Ok(true) => {}
        Ok(false) => return TypedToolResult::error("记忆不存在、已归档或已固定。"),
        Err(error) => return TypedToolResult::error(error.to_string()),
    }
    TypedToolResult::success(
        format!("记忆已归档。ID: {id}"),
        MemoryResult {
            action: "archive".to_string(),
            id: Some(id.to_string()),
            ..MemoryResult::default()
        },
    )
}

pub(super) async fn restore_memory(
    input: Value,
    port: &dyn MemoryPort,
) -> TypedToolResult<MemoryResult> {
    let id = match required_string(&input, "id")
        .and_then(|id| Id::new(id).map_err(|error| error.to_string()))
    {
        Ok(id) => id,
        Err(error) => return TypedToolResult::error(error),
    };
    match port.restore(&id).await {
        Ok(RestoreResult::Restored { id }) => TypedToolResult::success(
            format!("记忆已恢复。ID: {id}"),
            MemoryResult {
                action: "restore".to_string(),
                id: Some(id.to_string()),
                ..MemoryResult::default()
            },
        ),
        Ok(RestoreResult::NeedsEviction { candidates }) => TypedToolResult::success(
            render_eviction_candidates(&candidates),
            MemoryResult {
                action: "needs_eviction".to_string(),
                id: Some(id.to_string()),
                eviction_candidates: Some(
                    candidates.iter().map(eviction_candidate_result).collect(),
                ),
                ..MemoryResult::default()
            },
        ),
        Ok(RestoreResult::NotFound) => TypedToolResult::error("归档记忆不存在。"),
        Ok(RestoreResult::NoOp) => TypedToolResult::success(
            "记忆恢复已跳过。",
            MemoryResult {
                action: "noop".to_string(),
                id: Some(id.to_string()),
                ..MemoryResult::default()
            },
        ),
        Err(error) => TypedToolResult::error(error.to_string()),
    }
}

fn render_eviction_candidates(candidates: &[EvictionCandidate]) -> String {
    if candidates.is_empty() {
        return "记忆数量已达上限，且没有可归档的非固定候选。".to_string();
    }
    let details = candidates
        .iter()
        .map(|candidate| {
            format!(
                "- id={} layer={} category={} confirmation_count={} last_confirmed_at={} eviction_score={} reason={}\n  {}",
                candidate.entry.id,
                memory_layer_name(candidate.entry.layer),
                memory_category_name(candidate.entry.category),
                candidate.entry.confirmation_count,
                candidate.entry.last_confirmed_at,
                candidate.eviction_score,
                candidate.eviction_reason,
                candidate.entry.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("记忆数量已达上限；写入未发生。请先使用 archive 显式归档候选，再重试：\n{details}")
}

fn eviction_candidate_result(candidate: &EvictionCandidate) -> MemoryEvictionCandidateResult {
    MemoryEvictionCandidateResult {
        id: candidate.entry.id.to_string(),
        content: candidate.entry.content.clone(),
        layer: memory_layer_result(candidate.entry.layer),
        category: memory_category_result(candidate.entry.category),
        tags: candidate.entry.tags.clone(),
        pinned: candidate.entry.pinned,
        outdated: candidate.entry.outdated,
        ttl_expired: candidate.ttl_expired,
        confirmation_count: candidate.entry.confirmation_count,
        last_confirmed_at: candidate.entry.last_confirmed_at,
        eviction_score: candidate.eviction_score,
        eviction_reason: candidate.eviction_reason.clone(),
    }
}

fn render_search_hits(hits: &[MemorySearchHit]) -> String {
    if hits.is_empty() {
        return "暂无记忆。".to_string();
    }
    hits.iter()
        .map(|hit| {
            let entry = &hit.entry;
            format!(
                "- id={} layer={} category={} tags={} location={} pinned={} outdated={} ttl_expired={} relevance={:.6}\n  {}",
                entry.id,
                memory_layer_name(entry.layer),
                memory_category_name(entry.category),
                render_tags(&entry.tags),
                match hit.location {
                    MemoryLocation::Active => "active",
                    MemoryLocation::Archive => "archive",
                },
                entry.pinned,
                hit.outdated,
                hit.ttl_expired,
                hit.relevance.unwrap_or_default(),
                entry.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_memory_entries(entries: &[MemoryEntry], now: u64) -> String {
    if entries.is_empty() {
        return "暂无记忆。".to_string();
    }
    entries
        .iter()
        .map(|entry| {
            format!(
                "- id={} layer={} category={} tags={} pinned={} outdated={} ttl_expired={}\n  {}",
                entry.id,
                memory_layer_name(entry.layer),
                memory_category_name(entry.category),
                render_tags(&entry.tags),
                entry.pinned,
                entry.outdated,
                entry.is_ttl_expired(now),
                entry.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_tags(tags: &[String]) -> String {
    if tags.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", tags.join(","))
    }
}

fn memory_layer_name(layer: Layer) -> &'static str {
    match layer {
        Layer::Global => "global",
        Layer::Project => "project",
    }
}

fn memory_category_name(category: Category) -> &'static str {
    match category {
        Category::Fact => "fact",
        Category::Decision => "decision",
        Category::Preference => "preference",
        Category::Pattern => "pattern",
        Category::Pitfall => "pitfall",
    }
}

fn memory_entry_result(entry: &MemoryEntry, now: u64) -> MemoryEntryResult {
    MemoryEntryResult {
        id: entry.id.to_string(),
        content: entry.content.clone(),
        layer: memory_layer_result(entry.layer),
        category: memory_category_result(entry.category),
        tags: entry.tags.clone(),
        pinned: entry.pinned,
        outdated: entry.outdated,
        ttl_expired: entry.is_ttl_expired(now),
    }
}

fn search_hit_result(hit: &MemorySearchHit) -> MemorySearchHitResult {
    MemorySearchHitResult {
        id: hit.entry.id.to_string(),
        content: hit.entry.content.clone(),
        layer: memory_layer_result(hit.entry.layer),
        category: memory_category_result(hit.entry.category),
        tags: hit.entry.tags.clone(),
        pinned: hit.entry.pinned,
        location: match hit.location {
            MemoryLocation::Active => MemoryLocationResult::Active,
            MemoryLocation::Archive => MemoryLocationResult::Archive,
        },
        outdated: hit.outdated,
        ttl_expired: hit.ttl_expired,
        relevance: hit.relevance,
    }
}

fn memory_layer_result(layer: Layer) -> MemoryLayerInput {
    match layer {
        Layer::Global => MemoryLayerInput::Global,
        Layer::Project => MemoryLayerInput::Project,
    }
}

fn memory_category_result(category: Category) -> MemoryCategoryInput {
    match category {
        Category::Fact => MemoryCategoryInput::Fact,
        Category::Decision => MemoryCategoryInput::Decision,
        Category::Preference => MemoryCategoryInput::Preference,
        Category::Pattern => MemoryCategoryInput::Pattern,
        Category::Pitfall => MemoryCategoryInput::Pitfall,
    }
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
                        id: Some(id),
                        ..MemoryResult::default()
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
                    id: Some(id.to_string()),
                    ..MemoryResult::default()
                },
            ),
            Err(error) => TypedToolResult::error(error.to_string()),
        },
        Err(_) => TypedToolResult::error("session reminder 状态锁已损坏"),
    };
    result
}

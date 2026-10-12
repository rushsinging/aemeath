use crate::domain::types::memory::{
    MemoryCategoryInput, MemoryEntryResult, MemoryEvictionCandidateResult, MemoryLayerInput,
    MemoryLocationResult, MemoryResult, MemorySearchHitResult, MemoryStatus,
};
use crate::domain::{ToolExecutionContext, TypedToolResult};
use memory::api::search::MemorySearchHit;
use memory::api::search::MemorySearchQuery as Query;
use memory::api::{
    EvictionCandidate, MemoryCategory as Category, MemoryEntry, MemoryId as Id,
    MemoryLayer as Layer, MemoryLocation, MemoryPort, MemorySource as Source, RestoreResult,
    WriteResult,
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

pub(super) async fn search_memory(
    input: Value,
    port: &dyn MemoryPort,
) -> TypedToolResult<MemoryResult> {
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
        limit: limit.min(super::constants::MEMORY_PAGE_MAX_LIMIT),
        layer,
        category,
        include_archive: false,
        now,
    };
    let result = port.search(&query).await;
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

pub(super) async fn update_memory(
    id: &str,
    status: MemoryStatus,
    port: &dyn MemoryPort,
) -> TypedToolResult<MemoryResult> {
    let id = match Id::new(id).map_err(|error| error.to_string()) {
        Ok(id) => id,
        Err(error) => return TypedToolResult::error(error),
    };
    match status {
        MemoryStatus::Pin => update_pin(&id, true, port).await,
        MemoryStatus::Unpin => update_pin(&id, false, port).await,
        MemoryStatus::Archive => update_archive(&id, port).await,
        MemoryStatus::Restore => update_restore(&id, port).await,
    }
}

async fn update_pin(id: &Id, pinned: bool, port: &dyn MemoryPort) -> TypedToolResult<MemoryResult> {
    match port.pin(id, pinned).await {
        Ok(true) => TypedToolResult::success(
            if pinned {
                "记忆已固定。"
            } else {
                "记忆已取消固定。"
            },
            MemoryResult {
                action: if pinned {
                    "pin".to_string()
                } else {
                    "unpin".to_string()
                },
                id: Some(id.to_string()),
                ..MemoryResult::default()
            },
        ),
        Ok(false) => TypedToolResult::error("记忆不存在。"),
        Err(error) => TypedToolResult::error(error.to_string()),
    }
}

async fn update_archive(id: &Id, port: &dyn MemoryPort) -> TypedToolResult<MemoryResult> {
    match port.archive(std::slice::from_ref(id)).await {
        Ok(true) => TypedToolResult::success(
            format!("记忆已归档。ID: {id}"),
            MemoryResult {
                action: "archive".to_string(),
                id: Some(id.to_string()),
                ..MemoryResult::default()
            },
        ),
        Ok(false) => TypedToolResult::error("记忆不存在、已归档或已固定。"),
        Err(error) => TypedToolResult::error(error.to_string()),
    }
}

async fn update_restore(id: &Id, port: &dyn MemoryPort) -> TypedToolResult<MemoryResult> {
    match port.restore(id).await {
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

pub(super) async fn list_memory(
    input: Value,
    port: &dyn MemoryPort,
) -> TypedToolResult<MemoryResult> {
    use super::constants::{MEMORY_LIST_DEFAULT_LIMIT, MEMORY_PAGE_MAX_LIMIT};

    let layer = match optional_layer(&input) {
        Ok(layer) => layer,
        Err(error) => return TypedToolResult::error(error),
    };
    let limit = input
        .get("limit")
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .filter(|value| *value > 0)
        .unwrap_or(MEMORY_LIST_DEFAULT_LIMIT)
        .min(MEMORY_PAGE_MAX_LIMIT);
    let offset = input
        .get("offset")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let all_entries = port.list(layer).await;
    let total = all_entries.len();
    let page: Vec<_> = all_entries.into_iter().skip(offset).take(limit).collect();
    let has_more = offset.saturating_add(page.len()) < total;
    let now = current_timestamp_secs();
    let message = render_memory_entries(&page, total, offset, limit, has_more, now);
    TypedToolResult::success(
        message,
        MemoryResult {
            action: "list".to_string(),
            entries: Some(
                page.iter()
                    .map(|entry| memory_entry_result(entry, now))
                    .collect(),
            ),
            total: Some(total as u64),
            offset: Some(offset as u64),
            limit: Some(limit as u64),
            has_more: Some(has_more),
            ..MemoryResult::default()
        },
    )
}

fn render_eviction_candidates(candidates: &[EvictionCandidate]) -> String {
    if candidates.is_empty() {
        return "记忆数量已达上限，且没有可归档的非固定候选。".to_string();
    }
    let header = format!(
        "记忆数量已达上限；写入未发生。请先 archive 候选再重试（{} 条）",
        candidates.len()
    );
    let lines = candidates.iter().map(|candidate| {
        format!(
            "{} · score {} · {} · {}",
            candidate.entry.id,
            candidate.eviction_score,
            candidate.eviction_reason,
            preview_content(&candidate.entry.content)
        )
    });
    render_bounded_lines(header, lines)
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
    let header = format!("命中 {} 条", hits.len());
    let lines = hits.iter().map(|hit| {
        let entry = &hit.entry;
        let location = match hit.location {
            MemoryLocation::Active => "active",
            MemoryLocation::Archive => "archive",
        };
        let relevance = hit
            .relevance
            .map(|score| format!("{score:.1}"))
            .unwrap_or_else(|| "-".to_string());
        format!(
            "{} · {} · {} · {} · {} · {}",
            entry.id,
            memory_layer_name(entry.layer),
            memory_category_name(entry.category),
            location,
            relevance,
            preview_content(&entry.content)
        )
    });
    render_bounded_lines(header, lines)
}

fn render_memory_entries(
    entries: &[MemoryEntry],
    total: usize,
    offset: usize,
    limit: usize,
    has_more: bool,
    now: u64,
) -> String {
    if total == 0 {
        return "暂无记忆。".to_string();
    }
    if entries.is_empty() {
        return format!("共 {total} 条，offset={offset} 超出范围");
    }
    let start = offset.saturating_add(1);
    let end = offset.saturating_add(entries.len());
    let mut header = format!("共 {total} 条，显示 {start}–{end}（limit={limit}）");
    if has_more {
        header.push_str(&format!(
            "；has_more next_offset={}",
            offset.saturating_add(entries.len())
        ));
    }
    let lines = entries
        .iter()
        .map(|entry| format_entry_summary_line(entry, now));
    render_bounded_lines(header, lines)
}

fn format_entry_summary_line(entry: &MemoryEntry, now: u64) -> String {
    format!(
        "{} · {} · {} · {} · {}",
        entry.id,
        memory_layer_name(entry.layer),
        memory_category_name(entry.category),
        entry_flags(
            entry.pinned,
            entry.outdated,
            entry.is_ttl_expired(now),
            entry.superseded_by.is_some()
        ),
        preview_content(&entry.content)
    )
}

fn entry_flags(pinned: bool, outdated: bool, ttl_expired: bool, superseded: bool) -> String {
    let mut flags = Vec::new();
    if pinned {
        flags.push("pinned");
    }
    if outdated {
        flags.push("outdated");
    }
    if ttl_expired {
        flags.push("ttl_expired");
    }
    if superseded {
        flags.push("superseded");
    }
    if flags.is_empty() {
        "-".to_string()
    } else {
        flags.join(",")
    }
}

fn preview_content(content: &str) -> String {
    use super::constants::MEMORY_RESULT_CONTENT_PREVIEW_CHARS;
    let max = MEMORY_RESULT_CONTENT_PREVIEW_CHARS;
    if content.chars().count() <= max {
        return content.to_string();
    }
    let truncated: String = content.chars().take(max.saturating_sub(1)).collect();
    format!("{truncated}…")
}

fn render_bounded_lines(header: String, lines: impl IntoIterator<Item = String>) -> String {
    use super::super::constants::{
        MEMORY_RESULT_HEADER_RESERVE_CHARS, MEMORY_RESULT_TEXT_BUDGET_CHARS,
    };

    let body_budget =
        MEMORY_RESULT_TEXT_BUDGET_CHARS.saturating_sub(MEMORY_RESULT_HEADER_RESERVE_CHARS);
    let mut used_chars = 0usize;
    let mut body = Vec::new();
    let mut omitted = false;
    for line in lines {
        let line_chars = line.chars().count().saturating_add(1);
        if used_chars.saturating_add(line_chars) > body_budget {
            omitted = true;
            break;
        }
        used_chars = used_chars.saturating_add(line_chars);
        body.push(line);
    }
    let mut header = header;
    if omitted {
        header.push_str("；文本已截断");
    }
    if body.is_empty() {
        return clip_header_to_budget(header);
    }
    let text = format!("{header}\n{}", body.join("\n"));
    if text.chars().count() <= MEMORY_RESULT_TEXT_BUDGET_CHARS {
        text
    } else {
        // 极端短预算兜底：保留 header。
        clip_header_to_budget(header)
    }
}

fn clip_header_to_budget(header: String) -> String {
    use super::super::constants::MEMORY_RESULT_TEXT_BUDGET_CHARS;
    if header.chars().count() <= MEMORY_RESULT_TEXT_BUDGET_CHARS {
        return header;
    }
    header
        .chars()
        .take(MEMORY_RESULT_TEXT_BUDGET_CHARS)
        .collect()
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
        superseded_by: entry.superseded_by.map(|id| id.to_string()),
        evidence: entry.evidence.iter().map(|id| id.to_string()).collect(),
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
        superseded_by: hit.superseded_by.map(|id| id.to_string()),
        evidence: hit.entry.evidence.iter().map(|id| id.to_string()).collect(),
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

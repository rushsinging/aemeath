//! Memory 工具族 TUI 显示：五工具专属 header + typed result 解析渲染
//! （List/Search 逐行摘要 / Add·Update·Delete action·id），替代 fallback 的 JSON 原文截断。

use super::super::common::{truncate_ellipsis, typed_from_content};
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use super::helpers::format_goal_header;
use sdk::tool_input::{
    MemoryAddInput, MemoryCategoryInput, MemoryDeleteInput, MemoryLayerInput, MemoryListInput,
    MemorySearchInput,
};
use sdk::tool_result::{
    MemoryEntryResult, MemoryEvictionCandidateResult, MemoryLocationResult, MemoryResult,
    MemorySearchHitResult,
};
use std::path::Path;

fn parse_input<T: serde::de::DeserializeOwned>(input: &serde_json::Value) -> Option<T> {
    serde_json::from_value(input.clone()).ok()
}

fn family_policy(max_lines: usize) -> ToolRenderPolicy {
    ToolRenderPolicy {
        header: HeaderPolicy::Compact,
        details: DetailsPolicy::Expanded,
        result: ResultPolicy::Visible {
            max_lines: Some(max_lines),
            render_kind: ResultRender::Plain,
            tail_mode: false,
        },
    }
}

fn layer_label(layer: MemoryLayerInput) -> &'static str {
    match layer {
        MemoryLayerInput::Global => "global",
        MemoryLayerInput::Project => "project",
    }
}

fn category_label(category: MemoryCategoryInput) -> &'static str {
    match category {
        MemoryCategoryInput::Fact => "fact",
        MemoryCategoryInput::Decision => "decision",
        MemoryCategoryInput::Preference => "preference",
        MemoryCategoryInput::Pattern => "pattern",
        MemoryCategoryInput::Pitfall => "pitfall",
    }
}

fn location_label(location: MemoryLocationResult) -> &'static str {
    match location {
        MemoryLocationResult::Active => "active",
        MemoryLocationResult::Archive => "archive",
    }
}

fn content_preview(content: &str) -> String {
    truncate_ellipsis(
        content,
        super::super::constants::MEMORY_CONTENT_PREVIEW_CHARS,
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

fn format_entry_line(entry: &MemoryEntryResult) -> String {
    format!(
        "{} · {} · {} · {} · {}",
        entry.id,
        layer_label(entry.layer),
        category_label(entry.category),
        entry_flags(
            entry.pinned,
            entry.outdated,
            entry.ttl_expired,
            entry.superseded_by.is_some()
        ),
        content_preview(&entry.content)
    )
}

fn format_hit_line(hit: &MemorySearchHitResult) -> String {
    let relevance = match hit.relevance {
        Some(score) => format!("{score:.1}"),
        None => "-".to_string(),
    };
    format!(
        "{} · {} · {} · {} · {} · {}",
        hit.id,
        layer_label(hit.layer),
        category_label(hit.category),
        location_label(hit.location),
        relevance,
        content_preview(&hit.content)
    )
}

fn format_eviction_line(candidate: &MemoryEvictionCandidateResult) -> String {
    format!(
        "{} · score {} · {} · {}",
        candidate.id,
        candidate.eviction_score,
        candidate.eviction_reason,
        content_preview(&candidate.content)
    )
}

fn format_action_result(result: &MemoryResult) -> Vec<String> {
    if result.action == "needs_eviction" {
        let mut lines = vec!["needs_eviction".to_string()];
        if let Some(candidates) = &result.eviction_candidates {
            lines.extend(candidates.iter().map(format_eviction_line));
        }
        return lines;
    }
    let id = result.id.as_deref().unwrap_or("-");
    vec![format!("{} · {}", result.action, id)]
}

fn append_filter_parts(
    parts: &mut Vec<String>,
    layer: Option<MemoryLayerInput>,
    category: Option<MemoryCategoryInput>,
) {
    if let Some(layer) = layer {
        parts.push(layer_label(layer).to_string());
    }
    if let Some(category) = category {
        parts.push(category_label(category).to_string());
    }
}

// ── MemoryAdd ──────────────────────────────────────────────────────

struct MemoryAddDisplay;
impl ToolDisplay for MemoryAddDisplay {
    fn name(&self) -> &str {
        "MemoryAdd"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let Some(args) = parse_input::<MemoryAddInput>(input) else {
            return self.display_name().to_string();
        };
        let mut fallback_parts = Vec::new();
        append_filter_parts(&mut fallback_parts, args.layer, args.category);
        let preview = content_preview(args.content.trim());
        if !preview.is_empty() {
            fallback_parts.push(preview);
        }
        format_goal_header(self.display_name(), &args.goal, &fallback_parts.join(" "))
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let Some(args) = parse_input::<MemoryAddInput>(input) else {
            return vec![];
        };
        let mut lines = Vec::new();
        append_filter_parts(&mut lines, args.layer, args.category);
        let preview = content_preview(args.content.trim());
        if !preview.is_empty() {
            lines.push(preview);
        }
        lines
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: MemoryResult = typed_from_content(content)?;
        Some(format_action_result(&result))
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(8)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "MemoryAdd",
    display: || Box::new(MemoryAddDisplay)
});

// ── MemorySearch ───────────────────────────────────────────────────

struct MemorySearchDisplay;
impl ToolDisplay for MemorySearchDisplay {
    fn name(&self) -> &str {
        "MemorySearch"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let Some(args) = parse_input::<MemorySearchInput>(input) else {
            return self.display_name().to_string();
        };
        let mut fallback_parts = Vec::new();
        append_filter_parts(&mut fallback_parts, args.layer, args.category);
        let query = args.query.trim();
        if !query.is_empty() {
            fallback_parts.push(content_preview(query));
        }
        format_goal_header(self.display_name(), &args.goal, &fallback_parts.join(" "))
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let Some(args) = parse_input::<MemorySearchInput>(input) else {
            return vec![];
        };
        let mut lines = Vec::new();
        append_filter_parts(&mut lines, args.layer, args.category);
        let query = args.query.trim();
        if !query.is_empty() {
            lines.push(query.to_string());
        }
        lines
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: MemoryResult = typed_from_content(content)?;
        let hits = result.hits.unwrap_or_default();
        if hits.is_empty() {
            return Some(vec!["（无命中）".to_string()]);
        }
        Some(hits.iter().map(format_hit_line).collect())
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(15)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "MemorySearch",
    display: || Box::new(MemorySearchDisplay)
});

// ── MemoryList ─────────────────────────────────────────────────────

struct MemoryListDisplay;
impl ToolDisplay for MemoryListDisplay {
    fn name(&self) -> &str {
        "MemoryList"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let Some(args) = parse_input::<MemoryListInput>(input) else {
            return self.display_name().to_string();
        };
        let mut fallback_parts = Vec::new();
        if let Some(layer) = args.layer {
            fallback_parts.push(layer_label(layer).to_string());
        }
        if let Some(offset) = args.offset {
            fallback_parts.push(format!("offset={offset}"));
        }
        if let Some(limit) = args.limit {
            fallback_parts.push(format!("limit={limit}"));
        }
        format_goal_header(self.display_name(), &args.goal, &fallback_parts.join(" "))
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let Some(args) = parse_input::<MemoryListInput>(input) else {
            return vec![];
        };
        let mut lines = Vec::new();
        if let Some(layer) = args.layer {
            lines.push(layer_label(layer).to_string());
        }
        if let Some(offset) = args.offset {
            lines.push(format!("offset={offset}"));
        }
        if let Some(limit) = args.limit {
            lines.push(format!("limit={limit}"));
        }
        lines
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: MemoryResult = typed_from_content(content)?;
        let entries = result.entries.unwrap_or_default();
        if entries.is_empty() {
            return Some(vec!["（无条目）".to_string()]);
        }
        let mut lines: Vec<String> = entries.iter().map(format_entry_line).collect();
        if result.has_more == Some(true) {
            let next = result
                .offset
                .unwrap_or(0)
                .saturating_add(result.limit.unwrap_or(entries.len() as u64));
            lines.push(format!("has_more next_offset={next}"));
        }
        Some(lines)
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(15)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "MemoryList",
    display: || Box::new(MemoryListDisplay)
});

// ── MemoryUpdate ───────────────────────────────────────────────────

struct MemoryUpdateDisplay;
impl ToolDisplay for MemoryUpdateDisplay {
    fn name(&self) -> &str {
        "MemoryUpdate"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        // MemoryUpdateInput.status 无 Debug/Display 且 MemoryStatus 未从 sdk
        // 再导出；fallback 直接读 JSON 字段，避免为显示再扩公开面。
        let goal = input
            .get("goal")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let id = input
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let status = input
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let fallback = match (id.is_empty(), status.is_empty()) {
            (false, false) => format!("{id} {status}"),
            (false, true) => id.to_string(),
            (true, false) => status.to_string(),
            (true, true) => String::new(),
        };
        format_goal_header(self.display_name(), goal, &fallback)
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(id) = input
            .get("id")
            .and_then(|value| value.as_str())
            .filter(|v| !v.is_empty())
        {
            lines.push(id.to_string());
        }
        if let Some(status) = input
            .get("status")
            .and_then(|value| value.as_str())
            .filter(|v| !v.is_empty())
        {
            lines.push(status.to_string());
        }
        lines
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: MemoryResult = typed_from_content(content)?;
        Some(format_action_result(&result))
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(8)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "MemoryUpdate",
    display: || Box::new(MemoryUpdateDisplay)
});

// ── MemoryDelete ───────────────────────────────────────────────────

struct MemoryDeleteDisplay;
impl ToolDisplay for MemoryDeleteDisplay {
    fn name(&self) -> &str {
        "MemoryDelete"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let Some(args) = parse_input::<MemoryDeleteInput>(input) else {
            return self.display_name().to_string();
        };
        format_goal_header(self.display_name(), &args.goal, &args.id)
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let Some(args) = parse_input::<MemoryDeleteInput>(input) else {
            return vec![];
        };
        if args.id.is_empty() {
            vec![]
        } else {
            vec![args.id.clone()]
        }
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: MemoryResult = typed_from_content(content)?;
        Some(format_action_result(&result))
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(8)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "MemoryDelete",
    display: || Box::new(MemoryDeleteDisplay)
});

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;

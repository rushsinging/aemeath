use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::super::common::{display_path, file_path, typed_data};
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use super::helpers::{build_header_line, truncate_path};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::path::Path;

// ── Read ─────────────────────────────────────────────────────────

struct ReadDisplay;

/// 计算 Read header 的 range_info 后缀。
///
/// - `actual_lines = Some(n)`（result 到达）：返回 `start:end (n lines)`
/// - `actual_lines = None`（running 中）：offset/limit 都默认时返回空字符串，
///   否则返回 `start:end`（预览范围）
fn read_range_info(input: &serde_json::Value, actual_lines: Option<usize>) -> String {
    let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;
    let start = offset + 1; // 转为 1-based
    match actual_lines {
        Some(actual) => {
            let actual_end = offset + actual;
            format!("{start}:{actual_end} ({actual} lines)")
        }
        None => {
            // running 中：只在用户显式传了 offset/limit 时显示预览范围，
            // 默认值（offset=0, limit=2000）时不显示，等 result 到来再展示实际范围。
            let has_explicit = input.get("offset").is_some() || input.get("limit").is_some();
            if has_explicit {
                format!("{start}:{}", offset + limit)
            } else {
                String::new()
            }
        }
    }
}

/// 构建 Read header 的 spans（name + path + 可选 range_info）。
fn read_header_spans(name: &str, display_path: &str, range_info: &str) -> Vec<Span<'static>> {
    let mut spans = vec![
        Span::styled(name.to_string(), Style::default().fg(theme::ACCENT_BRIGHT)),
        Span::raw(format!(" {display_path}")),
    ];
    if !range_info.is_empty() {
        spans.push(Span::raw(" ".to_string()));
        spans.push(Span::styled(
            range_info.to_string(),
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }
    spans
}

impl ToolDisplay for ReadDisplay {
    fn name(&self) -> &str {
        "Read"
    }
    fn format_header(&self, input: &serde_json::Value, workspace_root: Option<&Path>) -> String {
        let path = file_path(input);
        let rel = display_path(path, workspace_root);
        let display_path = truncate_path(&rel, 60);
        let range = read_range_info(input, None);
        if range.is_empty() {
            format!("{} {display_path}", self.display_name())
        } else {
            format!("{} {display_path} {range}", self.display_name())
        }
    }
    fn header_for_subagent(
        &self,
        input: &serde_json::Value,
        workspace_root: Option<&Path>,
    ) -> String {
        self.format_header(input, workspace_root)
    }
    fn format_header_line(
        &self,
        input: &serde_json::Value,
        workspace_root: Option<&Path>,
    ) -> Line<'static> {
        // 委托给 with_result（传 None），避免 range 逻辑重复。
        self.format_header_line_with_result(input, None, workspace_root)
    }
    /// 当 result 到达后，使用实际读取的行数更新 header。
    /// 从 `ReadResult.line_count` 反序列化读取；缺失时回退到 regex 解析。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let path = file_path(input);
        let rel = display_path(path, workspace_root);
        let display_path = truncate_path(&rel, 60);

        // typed 优先：ReadResult.line_count
        let actual_lines = typed_data::<sdk::tool_result::ReadResult>(result_payload)
            .map(|r| r.line_count as usize)
            // regex 回退：旧 ToolResult 仅 message 含 "Read N lines from ..."
            .or_else(|| result_payload.and_then(|p| parse_line_count_from_message(&p.output)));

        let range_info = read_range_info(input, actual_lines);
        Line::from(read_header_spans(
            self.display_name(),
            &display_path,
            &range_info,
        ))
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        // 行范围信息已在 header 中，不再需要 details
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Hidden, // 不显示 result 子块
        }
    }
}

/// 从 message 中解析行数，如 "Read 340 lines from /path/to/file"
fn parse_line_count_from_message(message: &str) -> Option<usize> {
    let re = regex::Regex::new(r"Read (\d+) lines? from").ok()?;
    re.captures(message)
        .and_then(|cap| cap.get(1))
        .and_then(|m| m.as_str().parse::<usize>().ok())
}
inventory::submit!(ToolDisplayEntry {
    name: "Read",
    display: || Box::new(ReadDisplay)
});

// ── Write ────────────────────────────────────────────────────────

struct WriteDisplay;
impl ToolDisplay for WriteDisplay {
    fn name(&self) -> &str {
        "Write"
    }
    fn format_header(&self, input: &serde_json::Value, workspace_root: Option<&Path>) -> String {
        let path = file_path(input);
        let rel = display_path(path, workspace_root);
        let display_path = truncate_path(&rel, 60);
        // 从 input 的 content 计算字节数
        let bytes = input
            .get("content")
            .and_then(|v| v.as_str())
            .map(|s| s.len())
            .unwrap_or(0);
        format!("{} {display_path} {bytes} bytes", self.display_name())
    }
    fn header_for_subagent(
        &self,
        input: &serde_json::Value,
        workspace_root: Option<&Path>,
    ) -> String {
        self.format_header(input, workspace_root)
    }
    /// 当 result 到达后，使用实际写入的字节数更新 header。
    /// 从 `WriteResult.bytes_written` 反序列化读取；缺失时回退到 regex 解析。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let path = file_path(input);
        let rel = display_path(path, workspace_root);
        let display_path = truncate_path(&rel, 60);

        // typed 优先：WriteResult.bytes_written
        let actual_bytes = typed_data::<sdk::tool_result::WriteResult>(result_payload)
            .map(|r| r.bytes_written as usize)
            // regex 回退：旧 ToolResult 仅 message 含 "Wrote N bytes to ..."
            .or_else(|| result_payload.and_then(|p| parse_bytes_from_message(&p.output)));

        // 计算入参中的字节数（回退值）
        let input_bytes = input
            .get("content")
            .and_then(|v| v.as_str())
            .map(|s| s.len())
            .unwrap_or(0);

        let bytes = actual_bytes.unwrap_or(input_bytes);
        let bytes_info = format!("{bytes} bytes");

        Line::from(vec![
            Span::styled(
                self.display_name().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ),
            Span::raw(format!(" {display_path} ")),
            Span::styled(bytes_info, Style::default().fg(theme::TEXT_MUTED)),
        ])
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        // 字节数已在 summary 中，不再需要 details
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Hidden, // 不显示 result 子块
        }
    }
}

/// 从 message 中解析字节数，如 "Wrote 1234 bytes to /path"
fn parse_bytes_from_message(message: &str) -> Option<usize> {
    let re = regex::Regex::new(r"Wrote (\d+) bytes? to").ok()?;
    re.captures(message)
        .and_then(|cap| cap.get(1))
        .and_then(|m| m.as_str().parse::<usize>().ok())
}
inventory::submit!(ToolDisplayEntry {
    name: "Write",
    display: || Box::new(WriteDisplay)
});

// ── Edit ─────────────────────────────────────────────────────────

struct EditDisplay;
impl ToolDisplay for EditDisplay {
    fn name(&self) -> &str {
        "Edit"
    }
    fn format_header(&self, input: &serde_json::Value, workspace_root: Option<&Path>) -> String {
        let path = file_path(input);
        let rel = display_path(path, workspace_root);
        let display_path = truncate_path(&rel, 60);
        // 从 input 的 old_string/new_string 计算变更统计
        let old_len = input
            .get("old_string")
            .and_then(|v| v.as_str())
            .map(|s| s.len())
            .unwrap_or(0);
        let new_len = input
            .get("new_string")
            .and_then(|v| v.as_str())
            .map(|s| s.len())
            .unwrap_or(0);
        format!(
            "{} {display_path} Changed {old_len} -> {new_len} chars",
            self.display_name()
        )
    }
    fn header_for_subagent(
        &self,
        input: &serde_json::Value,
        workspace_root: Option<&Path>,
    ) -> String {
        self.format_header(input, workspace_root)
    }
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let path = file_path(input);
        let rel = display_path(path, workspace_root);
        let suffix = typed_data::<sdk::tool_result::EditResult>(result_payload)
            .map(|r| format!(" (Replaced {})", r.replacements_made))
            .unwrap_or_default();
        build_header_line(self.display_name(), &rel, &suffix)
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        // old/new 内容由 result 子块的 diff 渲染展示
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: None, // 全部显示
                render_kind: ResultRender::Diff,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Edit",
    display: || Box::new(EditDisplay)
});

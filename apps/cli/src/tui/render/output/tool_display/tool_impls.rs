use crate::tui::render::output_area::INDENT;
use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::common::{
    display_path, file_path, str_arg, truncate_ellipsis, truncate_ellipsis_tail, typed_data,
};
use super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::path::Path;
// ── Bash ─────────────────────────────────────────────────────────

struct BashDisplay;
impl ToolDisplay for BashDisplay {
    fn name(&self) -> &str {
        "Bash"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let cmd = str_arg(input, "command", "");
        // 命令可含任意 UTF-8（如中文 PR 标题），用宽度感知、char 边界安全的截断。
        if cmd.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {}", self.display_name(), truncate_ellipsis(cmd, 80))
        }
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let cmd = str_arg(input, "command", "");
        if cmd.is_empty() {
            return vec![];
        }
        // 截断显示，避免过长命令占用太多空间
        vec![truncate_ellipsis(
            cmd,
            200usize.saturating_sub(INDENT.len()),
        )]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Expanded,
            result: ResultPolicy::Visible {
                max_lines: Some(5),
                render_kind: ResultRender::Plain,
                tail_mode: true, // 只显示最后 5 行
            },
        }
    }
    /// 当 result 到达后，从 `BashResult.exit_code` / `signal` 读取
    /// exit code 显示后缀：0/None 空；signal 有值 `(signal N)`；> 0 `(exit N)`。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
        let result: Option<sdk::tool_result::BashResult> = typed_data(result_payload);
        let suffix = match result {
            Some(r) if r.exit_code != 0 => {
                if let Some(sig) = r.signal {
                    format!(" (signal {sig})")
                } else {
                    format!(" (exit {})", r.exit_code)
                }
            }
            _ => String::new(),
        };
        if cmd.is_empty() && suffix.is_empty() {
            Line::from(Span::styled(
                self.display_name().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ))
        } else if cmd.is_empty() {
            Line::from(vec![
                Span::styled(
                    self.display_name().to_string(),
                    Style::default().fg(theme::ACCENT_BRIGHT),
                ),
                Span::styled(suffix, Style::default().fg(theme::TEXT_MUTED)),
            ])
        } else {
            let display_cmd = truncate_ellipsis(cmd, 80);
            let mut spans = vec![
                Span::styled(
                    self.display_name().to_string(),
                    Style::default().fg(theme::ACCENT_BRIGHT),
                ),
                Span::raw(format!(" {display_cmd}")),
            ];
            if !suffix.is_empty() {
                spans.push(Span::styled(suffix, Style::default().fg(theme::TEXT_MUTED)));
            }
            Line::from(spans)
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Bash",
    display: || Box::new(BashDisplay)
});

// ── Read ─────────────────────────────────────────────────────────

/// 截断路径，保留尾部（更有辨识度）。路径可含非 ASCII（如中文文件名），
/// 故委托给 char 边界安全的 `truncate_ellipsis_tail`。
fn truncate_path(path: &str, max_width: usize) -> String {
    truncate_ellipsis_tail(path, max_width)
}

/// 构造 `ToolDisplay` 通用 header line 模板：`<name> <path> [<suffix>]`。
///
/// - `<name>`：工具 display name（如 "Read"），使用 `theme::ACCENT_BRIGHT` 高亮
/// - `<path>`：truncate_path 截断到 60 字符，使用 `theme::TEXT` 普通色
/// - `<suffix>`：可选尾部后缀（如 "1:340 (340 lines)"、"1234 bytes"），使用
///   `theme::TEXT_MUTED` 弱化色；空串则不输出后缀 span
///
/// 此 helper 是 Phase A Task 4 抽取：原 Read/Write/未来 9 个 Display 都重复
/// `Line::from(vec![Span::styled(name, ACCENT), Span::raw(" "), Span::styled(path, ...),
/// Span::styled(suffix, MUTED)])` 模板，DRY 化后由 helper 统一处理。
fn build_header_line(name: &str, path: &str, suffix: &str) -> Line<'static> {
    let display_path = truncate_path(path, 60);
    let mut spans = vec![Span::styled(
        name.to_string(),
        Style::default().fg(theme::ACCENT_BRIGHT),
    )];
    if !display_path.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(display_path, Style::default().fg(theme::TEXT)));
    }
    if !suffix.is_empty() {
        spans.push(Span::styled(
            suffix.to_string(),
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }
    Line::from(spans)
}

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

// ── Glob ─────────────────────────────────────────────────────────

struct GlobDisplay;
impl ToolDisplay for GlobDisplay {
    fn name(&self) -> &str {
        "Glob"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let pattern = str_arg(input, "pattern", "");
        if pattern.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {pattern}", self.display_name())
        }
    }
    /// result 到达后，从 `GlobResult.count` 反序列化读取匹配文件数。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let pattern = str_arg(input, "pattern", "");
        let n = typed_data::<sdk::tool_result::GlobResult>(result_payload).map(|r| r.count);
        let suffix = n.map(|c| format!(" ({c} files)")).unwrap_or_default();
        if pattern.is_empty() && suffix.is_empty() {
            Line::from(Span::styled(
                self.display_name().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ))
        } else {
            build_header_line(self.display_name(), pattern, &suffix)
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(5),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Glob",
    display: || Box::new(GlobDisplay)
});

// ── Grep ─────────────────────────────────────────────────────────

struct GrepDisplay;
impl ToolDisplay for GrepDisplay {
    fn name(&self) -> &str {
        "Grep"
    }
    fn format_header(&self, input: &serde_json::Value, workspace_root: Option<&Path>) -> String {
        let pattern = str_arg(input, "pattern", "");
        let path = str_arg(input, "path", ".");
        let rel = display_path(path, workspace_root);
        let display_path = truncate_path(&rel, 40);
        if pattern.is_empty() {
            format!("{} in {display_path}", self.display_name())
        } else {
            format!("{} /{pattern}/ in {display_path}", self.display_name())
        }
    }
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let rel = display_path(path, workspace_root);
        let arg = if pattern.is_empty() {
            format!("in {rel}")
        } else {
            format!("/{pattern}/, path={rel}")
        };
        let n = typed_data::<sdk::tool_result::GrepResult>(result_payload).map(|r| r.total_matches);
        let suffix = n.map(|c| format!(" ({c} matches)")).unwrap_or_default();
        build_header_line(self.display_name(), &arg, &suffix)
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(5),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Grep",
    display: || Box::new(GrepDisplay)
});

// ── Agent ────────────────────────────────────────────────────────

struct AgentDisplay;
impl ToolDisplay for AgentDisplay {
    fn name(&self) -> &str {
        "Agent"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let desc = str_arg(input, "description", "sub-task");
        let role = input.get("role").and_then(|role| role.as_str());
        let model = input.get("model").and_then(|model| model.as_str());
        let mut header = format!("{} {desc}", self.display_name());
        if let Some(r) = role {
            header.push_str(&format!(" [role: {r}]"));
        }
        if let Some(m) = model {
            header.push_str(&format!(" [model: {m}]"));
        }
        header
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let prompt = str_arg(input, "prompt", "");
        if prompt.is_empty() {
            return vec![];
        }
        vec![truncate_ellipsis(
            prompt,
            200usize.saturating_sub(INDENT.len()),
        )]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Expanded,
            result: ResultPolicy::Visible {
                max_lines: Some(5),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let target = typed_data::<sdk::tool_result::AgentResult>(result_payload)
            .and_then(|r| r.task_id)
            .filter(|id| !id.is_empty());
        let arg = match target {
            Some(id) => format!("{description} -> [{id}]"),
            None => description.to_string(),
        };
        build_header_line(self.display_name(), &arg, "")
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Agent",
    display: || Box::new(AgentDisplay)
});

// ── EnterWorktree ────────────────────────────────────────────────

struct EnterWorktreeDisplay;
impl ToolDisplay for EnterWorktreeDisplay {
    fn name(&self) -> &str {
        "EnterWorktree"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let target = input
            .get("branch")
            .and_then(|branch| branch.as_str())
            .or_else(|| input.get("path").and_then(|path| path.as_str()))
            .unwrap_or("worktree");
        format!("{} {target}", self.display_name())
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_header_line_with_result(
        &self,
        _input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let result: Option<sdk::tool_result::EnterWorktreeResult> = typed_data(result_payload);
        let branch = result
            .as_ref()
            .map(|r| r.branch.clone())
            .unwrap_or_else(|| "(default)".to_string());
        let arg = format!("branch={branch}");
        let path_suffix = result
            .map(|r| format!(" ({})", r.workspace_root.display()))
            .unwrap_or_default();
        build_header_line(self.display_name(), &arg, &path_suffix)
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(16),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "EnterWorktree",
    display: || Box::new(EnterWorktreeDisplay)
});

// ── ExitWorktree ─────────────────────────────────────────────────

struct ExitWorktreeDisplay;
impl ToolDisplay for ExitWorktreeDisplay {
    fn name(&self) -> &str {
        "ExitWorktree"
    }
    fn format_header(&self, _input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        self.display_name().to_string()
    }
    fn format_header_line_with_result(
        &self,
        _input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let path_suffix = typed_data::<sdk::tool_result::ExitWorktreeResult>(result_payload)
            .map(|r| format!(" (back to {})", r.workspace_root.display()))
            .unwrap_or_default();
        build_header_line(self.display_name(), "", &path_suffix)
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(16),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "ExitWorktree",
    display: || Box::new(ExitWorktreeDisplay)
});

// ── WebFetch ─────────────────────────────────────────────────────

struct WebFetchDisplay;
impl ToolDisplay for WebFetchDisplay {
    fn name(&self) -> &str {
        "WebFetch"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let url = str_arg(input, "url", "");
        if url.is_empty() {
            self.display_name().to_string()
        } else {
            let display_url = truncate_ellipsis(url, 60);
            format!("{} {display_url}", self.display_name())
        }
    }
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let url = input.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let result: Option<sdk::tool_result::WebFetchResult> = typed_data(result_payload);
        let suffix = result
            .filter(|r| r.truncated)
            .map(|_| " (truncated)".to_string())
            .unwrap_or_default();
        if url.is_empty() && suffix.is_empty() {
            Line::from(Span::styled(
                self.display_name().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ))
        } else {
            build_header_line(self.display_name(), url, &suffix)
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(5),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "WebFetch",
    display: || Box::new(WebFetchDisplay)
});

// ── AskUserQuestion ──────────────────────────────────────────────

struct AskUserQuestionDisplay;
impl ToolDisplay for AskUserQuestionDisplay {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }
    fn format_header(&self, _input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        // Issue #545: 不再把 question 截断拼进 header，避免长问题信息丢失。
        // 完整 question 由交互区域（blocks/ask_user.rs）按段落渲染。
        self.display_name().to_string()
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Hidden, // answer is already echoed via App::append_user_echo
        }
    }
    /// result 到达后，从 `AskUserQuestionResult.options` 读取选项数，
    /// suffix 形如 ` (N options)`。
    fn format_header_line_with_result(
        &self,
        _input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        // Issue #545: header 只显示 display_name + result suffix，不再带 question 预览。
        let n = typed_data::<sdk::tool_result::AskUserQuestionResult>(result_payload)
            .map(|r| r.options.len() as u64);
        let suffix = n.map(|c| format!(" ({c} options)")).unwrap_or_default();
        build_header_line(self.display_name(), "", &suffix)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "AskUserQuestion",
    display: || Box::new(AskUserQuestionDisplay)
});

#[cfg(test)]
mod tests {
    use super::build_header_line;

    /// 基础：name + path，suffix 为空时不应输出尾部 span
    #[test]
    fn build_header_line_no_suffix() {
        let line = build_header_line("Read", "/foo/bar/baz.txt", "");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "Read /foo/bar/baz.txt");
    }

    /// 有 suffix 时尾部追加弱化色 span
    #[test]
    fn build_header_line_with_suffix() {
        let line = build_header_line("Read", "/foo/bar/baz.txt", " (5 lines)");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "Read /foo/bar/baz.txt (5 lines)");
    }

    /// 长路径应触发 truncate_path 截断（带 ...）
    #[test]
    fn build_header_line_truncates_long_path() {
        // 90+ 字符的路径以确保超过 truncate_path(60) 阈值
        let long =
            "/very/very/very/very/very/very/very/very/very/very/very/very/very/long/path/file.txt";
        let line = build_header_line("Read", long, "");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.starts_with("Read "), "expected Read prefix: {text}");
        assert!(
            text.contains("..."),
            "expected ellipsis in long path: {text}"
        );
        assert!(
            text.len() < long.len() + 10,
            "long path should be truncated: {text}"
        );
    }

    // ── AgentDisplay::format_header_line_with_result 回归测试 (#422) ──

    use crate::tui::render::output::tool_display::ToolDisplay;
    use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
    use serde_json::json;

    fn agent_header_text(description: &str, result_content: Option<serde_json::Value>) -> String {
        let display = super::AgentDisplay;
        let input = json!({ "description": description });
        let payload = result_content.map(|c| ToolResultPayload::new(String::new(), c, false, 0));
        let line = display.format_header_line_with_result(&input, payload.as_ref(), None);
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// 有 task_id 时应显示 `-> [id]` 后缀
    #[test]
    fn agent_header_with_task_id_shows_suffix() {
        let text = agent_header_text(
            "do something",
            Some(json!({ "task_id": "task-42", "output": "done" })),
        );
        assert_eq!(text, "Agent do something -> [task-42]");
    }

    /// task_id 为 None 时不应显示 `-> []` 后缀（#422 回归）
    #[test]
    fn agent_header_without_task_id_hides_suffix() {
        let text = agent_header_text(
            "do something",
            Some(json!({ "task_id": null, "output": "done" })),
        );
        assert_eq!(text, "Agent do something");
    }

    /// task_id 为空字符串时也不应显示后缀
    #[test]
    fn agent_header_with_empty_task_id_hides_suffix() {
        let text = agent_header_text(
            "do something",
            Some(json!({ "task_id": "", "output": "done" })),
        );
        assert_eq!(text, "Agent do something");
    }

    // ── AskUserQuestionDisplay::format_header 回归测试 (#545) ──

    /// Issue #545: header 不应包含 question 截断预览
    #[test]
    fn ask_user_header_never_includes_question_preview() {
        let display = super::AskUserQuestionDisplay;
        let long_question =
            "这是一个非常非常非常长的问题，包含很多很多很多很多很多很多很多很多很多很多很多很多很多很多细节。";
        let input =
            json!({ "question": long_question, "options": [{"title": "a"}, {"title": "b"}] });
        let header = display.format_header(&input, None);
        assert_eq!(
            header, "Ask",
            "header 应只显示 display_name，不包含 question 截断预览"
        );
    }

    /// Issue #545: format_header_line_with_result 也不应将 question 当 path 截断
    #[test]
    fn ask_user_header_line_never_includes_question_preview() {
        let display = super::AskUserQuestionDisplay;
        let long_question =
            "这是一个非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常非常长的多段落问题。";
        let input =
            json!({ "question": long_question, "options": [{"title": "a"}, {"title": "b"}] });
        let result = serde_json::json!({
            "options": [{"title": "a", "description": ""}, {"title": "b", "description": ""}],
            "free_input": null,
        });
        let payload = ToolResultPayload::new(String::new(), result, false, 0);
        let line = display.format_header_line_with_result(&input, Some(&payload), None);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !text.contains("非常"),
            "header line 不应包含 question 内容: {text}"
        );
        assert!(
            text.starts_with("Ask"),
            "header line 应以 display_name 开头: {text}"
        );
    }
}

use crate::tui::render::output_area::INDENT;
use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;

use super::common::{file_path, str_arg, truncate_ellipsis, truncate_ellipsis_tail};
use super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// 从 `payload.content` (typed JSON Value) 中按 dotted path 提取 u64 字段。
///
/// path 形如 `"data.line_count"`，先走 `content` 的子对象查找再逐层下钻；
/// 任一节点缺失或类型不匹配返回 `None`。TUI 渲染层 inline helper，避免引入
/// 对 share/sdk 类型的硬依赖（typed 字段在 share::tool::types::read 中定义，
/// 但本模块不 import share，确保渲染层可独立编译与测试）。
fn data_field_u64(payload: Option<&ToolResultPayload>, path: &str) -> Option<u64> {
    let payload = payload?;
    let mut current: &serde_json::Value = &payload.content;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    current.as_u64()
}

/// 同 `data_field_u64`，但提取 i64 字段（Bash exit_code 等可为负值，标识 signal）。
fn data_field_i64(payload: Option<&ToolResultPayload>, path: &str) -> Option<i64> {
    let payload = payload?;
    let mut current: &serde_json::Value = &payload.content;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    current.as_i64()
}

/// 同 `data_field_u64`，但提取 String 字段（branch / agent_id / working_root 等）。
fn data_field_string(payload: Option<&ToolResultPayload>, path: &str) -> Option<String> {
    let payload = payload?;
    let mut current: &serde_json::Value = &payload.content;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    current.as_str().map(|s| s.to_string())
}

// ── Bash ─────────────────────────────────────────────────────────

struct BashDisplay;
impl ToolDisplay for BashDisplay {
    fn name(&self) -> &str {
        "Bash"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
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
    /// 当 result 到达后，从 `payload.content.data.exit_code` (i64) 读取
    /// exit code 显示后缀：0/None 空；< 0 `(signal N)`；> 0 `(exit N)`。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
    ) -> Line<'static> {
        let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
        let exit = data_field_i64(result_payload, "data.exit_code");
        let suffix = match exit {
            Some(0) | None => String::new(),
            Some(code) if code < 0 => format!(" (signal {})", -code),
            Some(code) => format!(" (exit {code})"),
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
    let mut spans = vec![
        Span::styled(
            name.to_string(),
            Style::default().fg(theme::ACCENT_BRIGHT),
        ),
        Span::raw(" "),
        Span::styled(display_path, Style::default().fg(theme::TEXT)),
    ];
    if !suffix.is_empty() {
        spans.push(Span::styled(
            suffix.to_string(),
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }
    Line::from(spans)
}

struct ReadDisplay;
impl ToolDisplay for ReadDisplay {
    fn name(&self) -> &str {
        "Read"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let path = file_path(input);
        let display_path = truncate_path(path, 60);
        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;
        let start = offset + 1; // 转为 1-based
        let end = offset + limit;
        format!("{} {display_path} {start}:{end}", self.display_name())
    }
    fn format_header_line(&self, input: &serde_json::Value) -> Line<'static> {
        let path = file_path(input);
        let display_path = truncate_path(path, 60);
        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;
        let start = offset + 1;
        let end = offset + limit;
        let range_info = format!("{start}:{end}");
        Line::from(vec![
            Span::styled(
                self.display_name().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ),
            Span::raw(format!(" {display_path} ")),
            Span::styled(range_info, Style::default().fg(theme::TEXT_MUTED)),
        ])
    }
    /// 当 result 到达后，使用实际读取的行数更新 header。
    /// typed 路径优先：直接从 `payload.content.data.line_count` 读取；
    /// typed 字段缺失时回退到原 regex 解析 `output` 文本（兼容旧 ToolResult）。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
    ) -> Line<'static> {
        let path = file_path(input);
        let display_path = truncate_path(path, 60);
        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;
        let start = offset + 1;

        // typed 优先：data.line_count (issue #273 引入的 typed R 字段)
        let actual_lines = data_field_u64(result_payload, "data.line_count")
            .map(|n| n as usize)
            // regex 回退：旧 ToolResult 仅 message 含 "Read N lines from ..."
            .or_else(|| {
                result_payload.and_then(|p| parse_line_count_from_message(&p.output))
            });

        let range_info = match actual_lines {
            Some(actual) => {
                let actual_end = offset + actual;
                format!("{start}:{actual_end} ({actual} lines)")
            }
            _ => {
                // 无法解析，不显示 () 部分
                let end = offset + limit;
                format!("{start}:{end}")
            }
        };

        Line::from(vec![
            Span::styled(
                self.display_name().to_string(),
                Style::default().fg(theme::ACCENT_BRIGHT),
            ),
            Span::raw(format!(" {display_path} ")),
            Span::styled(range_info, Style::default().fg(theme::TEXT_MUTED)),
        ])
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
    fn format_header(&self, input: &serde_json::Value) -> String {
        let path = file_path(input);
        let display_path = truncate_path(path, 60);
        // 从 input 的 content 计算字节数
        let bytes = input
            .get("content")
            .and_then(|v| v.as_str())
            .map(|s| s.len())
            .unwrap_or(0);
        format!("{} {display_path} {bytes} bytes", self.display_name())
    }
    /// 当 result 到达后，使用实际写入的字节数更新 header。
    /// typed 路径优先：直接从 `payload.content.data.bytes_written` 读取；
    /// typed 字段缺失时回退到原 regex 解析 `output` 文本（兼容旧 ToolResult）。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
    ) -> Line<'static> {
        let path = file_path(input);
        let display_path = truncate_path(path, 60);

        // typed 优先：data.bytes_written (issue #273 引入的 typed R 字段)
        let actual_bytes = data_field_u64(result_payload, "data.bytes_written")
            .map(|n| n as usize)
            // regex 回退：旧 ToolResult 仅 message 含 "Wrote N bytes to ..."
            .or_else(|| {
                result_payload.and_then(|p| parse_bytes_from_message(&p.output))
            });

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
    fn format_header(&self, input: &serde_json::Value) -> String {
        let path = file_path(input);
        let display_path = truncate_path(path, 60);
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
    ) -> Line<'static> {
        let path = file_path(input);
        let suffix = data_field_u64(result_payload, "data.occurrences")
            .map(|n| format!(" (Replaced {n})"))
            .unwrap_or_default();
        build_header_line(self.display_name(), path, &suffix)
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
    fn format_header(&self, input: &serde_json::Value) -> String {
        let pattern = str_arg(input, "pattern", "");
        if pattern.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {pattern}", self.display_name())
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
    fn format_header(&self, input: &serde_json::Value) -> String {
        let pattern = str_arg(input, "pattern", "");
        let path = str_arg(input, "path", ".");
        let display_path = truncate_path(path, 40);
        if pattern.is_empty() {
            format!("{} in {display_path}", self.display_name())
        } else {
            format!("{} /{pattern}/ in {display_path}", self.display_name())
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
    name: "Grep",
    display: || Box::new(GrepDisplay)
});

// ── Agent ────────────────────────────────────────────────────────

struct AgentDisplay;
impl ToolDisplay for AgentDisplay {
    fn name(&self) -> &str {
        "Agent"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
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
    fn format_header(&self, input: &serde_json::Value) -> String {
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
    ) -> Line<'static> {
        let branch = data_field_string(result_payload, "data.branch")
            .unwrap_or_else(|| "(default)".to_string());
        let arg = format!("branch={branch}");
        let path_suffix = data_field_string(result_payload, "data.working_root")
            .map(|p| format!(" ({p})"))
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
    fn format_header(&self, _input: &serde_json::Value) -> String {
        self.display_name().to_string()
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
    fn format_header(&self, input: &serde_json::Value) -> String {
        let url = str_arg(input, "url", "");
        if url.is_empty() {
            self.display_name().to_string()
        } else {
            let display_url = truncate_ellipsis(url, 60);
            format!("{} {display_url}", self.display_name())
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
    fn format_header(&self, input: &serde_json::Value) -> String {
        let question = str_arg(input, "question", "");
        if question.is_empty() {
            self.display_name().to_string()
        } else {
            let preview = truncate_ellipsis(question, 60usize);
            format!("{} {preview}", self.display_name())
        }
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
        let long = "/very/very/very/very/very/very/very/very/very/very/very/very/very/long/path/file.txt";
        let line = build_header_line("Read", long, "");
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.starts_with("Read "), "expected Read prefix: {text}");
        assert!(text.contains("..."), "expected ellipsis in long path: {text}");
        assert!(text.len() < long.len() + 10, "long path should be truncated: {text}");
    }
}

use crate::tui::render::output_area::INDENT;
use crate::tui::render::theme;

use super::common::{file_path, str_arg, truncate_ellipsis, truncate_ellipsis_tail};
use super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

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
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        // command 已在 header 显示，不再需要 details
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(5),
                render_kind: ResultRender::Plain,
                tail_mode: true, // 只显示最后 5 行
            },
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
        format!(
            "{} {display_path} L{start}:L{end} ({limit} lines)",
            self.display_name()
        )
    }
    fn format_header_line(&self, input: &serde_json::Value) -> Line<'static> {
        let path = file_path(input);
        let display_path = truncate_path(path, 60);
        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;
        let start = offset + 1;
        let end = offset + limit;
        let range_info = format!("L{start}:L{end} ({limit} lines)");
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
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_summary: Option<&str>,
    ) -> Line<'static> {
        let path = file_path(input);
        let display_path = truncate_path(path, 60);
        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;
        let start = offset + 1;

        // 尝试从 result_summary 中解析实际行数
        // result_summary 格式: "Read {n} lines from {path}" 或完整 JSON
        let actual_lines = result_summary.and_then(|summary| {
            // 尝试解析 JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(summary) {
                if let Some(message) = json.get("message").and_then(|v| v.as_str()) {
                    return parse_line_count_from_message(message);
                }
            }
            // 直接解析文本
            parse_line_count_from_message(summary)
        });

        let range_info = match actual_lines {
            Some(actual) if actual < limit => {
                // 实际行数小于请求的 limit，显示实际行数
                let actual_end = offset + actual;
                format!("L{start}:L{actual_end} ({actual} lines)")
            }
            _ => {
                // 无法解析或实际行数等于 limit，显示请求的 limit
                let end = offset + limit;
                format!("L{start}:L{end} ({limit} lines)")
            }
        };

        Line::from(vec![
            Span::raw(format!("{} {display_path} ", self.display_name())),
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
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        // 变更统计已在 summary 中，不再需要 details
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

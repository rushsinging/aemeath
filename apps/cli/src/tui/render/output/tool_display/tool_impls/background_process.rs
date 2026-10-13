//! Background Process 工具族 TUI 显示（#1895 / #1931）：五工具专属 header +
//! typed result 解析渲染（List 逐行摘要 / Status 详情 / Logs 多行原文 /
//! Stop 信号态 / Watch outcome），替代 fallback 的 JSON 原文截断。

use super::super::common::{typed_data, typed_from_content};
use super::super::{
    format_call_duration, DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay,
    ToolDisplayEntry, ToolRenderPolicy,
};
use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use sdk::tool_input::{
    BackgroundProcessLogsInput, BackgroundProcessStatusInput, BackgroundProcessStopInput,
    BackgroundProcessWatchInput,
};
use sdk::tool_result::{
    BackgroundProcessListResult, BackgroundProcessLogsResult, BackgroundProcessStatusResult,
    BackgroundProcessStopResult, BackgroundProcessWatchOutcome, BackgroundProcessWatchResult,
};
use std::path::Path;

fn parse_input<T: serde::de::DeserializeOwned + Default>(input: &serde_json::Value) -> T {
    serde_json::from_value(input.clone()).unwrap_or_default()
}

fn short_task_id(task_id: &str) -> String {
    use super::super::constants::BACKGROUND_TASK_ID_SHORT_CHARS;
    let id = task_id.trim();
    let chars: Vec<char> = id.chars().collect();
    if chars.len() <= BACKGROUND_TASK_ID_SHORT_CHARS {
        return id.to_string();
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(6)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

/// #1964：`Watch · {summary} · {短尾}`；无 summary 时 `Watch · {短尾}`。
fn watch_header(summary: &str, task_id: &str) -> String {
    let summary = summary.trim();
    let short = short_task_id(task_id);
    match (!summary.is_empty(), !short.is_empty()) {
        (true, true) => format!("Watch · {summary} · {short}"),
        (true, false) => format!("Watch · {summary}"),
        (false, true) => format!("Watch · {short}"),
        (false, false) => "Watch".to_string(),
    }
}

fn styled_watch_header(text: &str) -> Line<'static> {
    let name = "Watch";
    if let Some(rest) = text.strip_prefix(name) {
        Line::from(vec![
            Span::styled(name.to_string(), Style::default().fg(theme::ACCENT_BRIGHT)),
            Span::raw(rest.to_string()),
        ])
    } else {
        Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(theme::ACCENT_BRIGHT),
        ))
    }
}

/// 工具族公共 policy：result 可见 + Plain（行内容由 `format_result_lines`
/// typed 产出）。
fn family_policy(max_lines: usize, tail_mode: bool) -> ToolRenderPolicy {
    ToolRenderPolicy {
        header: HeaderPolicy::Compact,
        details: DetailsPolicy::Hidden,
        result: ResultPolicy::Visible {
            max_lines: Some(max_lines),
            render_kind: ResultRender::Plain,
            tail_mode,
        },
    }
}

// ── BackgroundProcessList ────────────────────────────────────────

struct BackgroundProcessListDisplay;
impl ToolDisplay for BackgroundProcessListDisplay {
    fn name(&self) -> &str {
        "BackgroundProcessList"
    }
    fn format_header(&self, _input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        self.display_name().to_string()
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: BackgroundProcessListResult = typed_from_content(content)?;
        if result.tasks.is_empty() {
            return Some(vec!["（无后台进程）".to_string()]);
        }
        Some(
            result
                .tasks
                .iter()
                .map(|task| {
                    format!(
                        "{} · {} · {} · {}",
                        task.task_id, task.tool_name, task.state, task.summary
                    )
                })
                .collect(),
        )
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(15, false)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "BackgroundProcessList",
    display: || Box::new(BackgroundProcessListDisplay)
});

// ── BackgroundProcessStatus ──────────────────────────────────────

struct BackgroundProcessStatusDisplay;
impl ToolDisplay for BackgroundProcessStatusDisplay {
    fn name(&self) -> &str {
        "BackgroundProcessStatus"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<BackgroundProcessStatusInput>(input);
        if args.task_id.is_empty() {
            return self.display_name().to_string();
        }
        format!("{} {}", self.display_name(), args.task_id)
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: BackgroundProcessStatusResult = typed_from_content(content)?;
        let detail = &result.detail;
        let deadline = match detail.deadline_remaining_ms {
            Some(remaining_ms) => {
                format!("deadline {}", format_call_duration(remaining_ms))
            }
            None => "no deadline".to_string(),
        };
        Some(vec![format!(
            "{} · {} · {} · {} · log bytes {} · {}",
            detail.summary.task_id,
            detail.summary.tool_name,
            detail.summary.state,
            deadline,
            detail.total_written_bytes,
            detail.summary.summary
        )])
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(8, false)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "BackgroundProcessStatus",
    display: || Box::new(BackgroundProcessStatusDisplay)
});

// ── BackgroundProcessLogs ────────────────────────────────────────

struct BackgroundProcessLogsDisplay;
impl ToolDisplay for BackgroundProcessLogsDisplay {
    fn name(&self) -> &str {
        "BackgroundProcessLogs"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<BackgroundProcessLogsInput>(input);
        if args.task_id.is_empty() {
            return self.display_name().to_string();
        }
        format!("{} {}", self.display_name(), args.task_id)
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: BackgroundProcessLogsResult = typed_from_content(content)?;
        // 多行日志原文渲染（不再经 JSON 转义展示）；空段返回提示行。
        if result.log.text.is_empty() {
            return Some(vec!["（无新增输出）".to_string()]);
        }
        Some(result.log.text.lines().map(str::to_string).collect())
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        // 放宽行数上限：日志本体多行；tail 模式只看最新段。
        // Logs 原文 tail：与 Bash 输出卡片同源常量（用户要求一致，
        // NEVER 各自另造数字）；完整日志经 cursor 增量读取。
        family_policy(super::super::constants::TOOL_RESULT_TAIL_LINES, true)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "BackgroundProcessLogs",
    display: || Box::new(BackgroundProcessLogsDisplay)
});

// ── BackgroundProcessStop ────────────────────────────────────────

struct BackgroundProcessStopDisplay;
impl ToolDisplay for BackgroundProcessStopDisplay {
    fn name(&self) -> &str {
        "BackgroundProcessStop"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<BackgroundProcessStopInput>(input);
        if args.task_id.is_empty() {
            return self.display_name().to_string();
        }
        format!("{} {}", self.display_name(), args.task_id)
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: BackgroundProcessStopResult = typed_from_content(content)?;
        let signal = if result.stop.signal_sent {
            "signal_sent"
        } else {
            "already_terminal"
        };
        Some(vec![format!("{} → {}", signal, result.stop.state)])
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        family_policy(3, false)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "BackgroundProcessStop",
    display: || Box::new(BackgroundProcessStopDisplay)
});

// ── BackgroundProcessWatch ──────────────────────────────

struct BackgroundProcessWatchDisplay;

impl ToolDisplay for BackgroundProcessWatchDisplay {
    fn name(&self) -> &str {
        "BackgroundProcessWatch"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<BackgroundProcessWatchInput>(input);
        // 等待中尚无账本 summary：仅短尾。
        watch_header("", &args.task_id)
    }
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        _workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let args = parse_input::<BackgroundProcessWatchInput>(input);
        let summary = typed_data::<BackgroundProcessWatchResult>(result_payload)
            .map(|result| result.detail.summary.summary)
            .unwrap_or_default();
        styled_watch_header(&watch_header(&summary, &args.task_id))
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let result: BackgroundProcessWatchResult = typed_from_content(content)?;
        let outcome = match result.outcome {
            BackgroundProcessWatchOutcome::Terminal => "terminal",
            BackgroundProcessWatchOutcome::StillRunning => "still_running",
            BackgroundProcessWatchOutcome::Cancelled => "cancelled",
        };
        let detail = &result.detail;
        let duration = match detail.summary.duration_ms {
            Some(elapsed_ms) => format_call_duration(elapsed_ms),
            None => "-".to_string(),
        };
        // log_tail 在前、outcome 摘要在后：tail_mode 保留最新日志的同时
        // NEVER 把 still_running/terminal 行裁掉。
        let mut lines = Vec::new();
        if !result.log_tail.is_empty() {
            lines.extend(result.log_tail.lines().map(str::to_string));
        }
        lines.push(format!(
            "{} · {} · {} · {} · {}",
            outcome, detail.summary.task_id, detail.summary.state, duration, detail.summary.summary
        ));
        Some(lines)
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        // log tail + 末行 outcome（与 Logs 同源尾部行数）。
        family_policy(super::super::constants::TOOL_RESULT_TAIL_LINES + 1, true)
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "BackgroundProcessWatch",
    display: || Box::new(BackgroundProcessWatchDisplay)
});

#[cfg(test)]
#[path = "background_process_tests.rs"]
mod tests;

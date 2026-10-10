//! Background Process 工具族 TUI 显示（#1895）：四工具专属 header +
//! typed result 解析渲染（List 逐行摘要 / Status 详情 / Logs 多行原文 /
//! Stop 信号态），替代 fallback 的 JSON 原文截断。

use super::super::common::typed_from_content;
use super::super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use sdk::tool_input::{
    BackgroundProcessLogsInput, BackgroundProcessStatusInput, BackgroundProcessStopInput,
};
use sdk::tool_result::{
    BackgroundProcessListResult, BackgroundProcessLogsResult, BackgroundProcessStatusResult,
    BackgroundProcessStopResult,
};
use std::path::Path;

fn parse_input<T: serde::de::DeserializeOwned + Default>(input: &serde_json::Value) -> T {
    serde_json::from_value(input.clone()).unwrap_or_default()
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
            Some(ms) => format!("deadline {}ms", ms),
            None => "no deadline".to_string(),
        };
        Some(vec![format!(
            "{} · {} · {} · {} · log bytes {}",
            detail.summary.task_id,
            detail.summary.tool_name,
            detail.summary.state,
            deadline,
            detail.total_written_bytes
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
        family_policy(40, true)
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

#[cfg(test)]
#[path = "background_process_tests.rs"]
mod tests;

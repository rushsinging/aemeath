use std::collections::HashMap;
use std::sync::LazyLock;

use crate::tui::output_area::{LineStyle, OutputLine, INDENT};

mod agent;
mod common;
mod results;
mod task_impls;
mod tool_impls;

use common::{format_todowrite_value, truncate_json};

/// TUI 中 tool call 结果最多显示的行数。
pub(crate) const TOOL_RESULT_MAX_LINES: usize = 5;

// ── ToolDisplay trait ──────────────────────────────────────────────

/// Trait for customizing how a tool call is displayed in the TUI output area.
pub trait ToolDisplay: Send + Sync {
    /// Tool name as registered in the tool registry.
    fn name(&self) -> &str;

    /// Format the header line (prefixed with ● by caller).
    fn format_header(&self, input: &serde_json::Value) -> String;

    /// Format detail lines shown below the header.
    fn format_details(&self, input: &serde_json::Value) -> Vec<String>;

    /// Style for detail lines.
    fn detail_style(&self) -> LineStyle {
        LineStyle::System
    }

    /// Max lines of result output to show (default 5).
    fn result_max_lines(&self) -> usize {
        TOOL_RESULT_MAX_LINES
    }

    /// Style for result content lines.
    fn result_style(&self) -> LineStyle {
        LineStyle::System
    }

    /// Format the result summary line(s). Default: "✓ {name} completed".
    fn format_result_summary(&self, _result: &str, is_error: bool) -> Vec<String> {
        if is_error {
            vec![format!("✗ {} failed", self.name())]
        } else {
            vec![format!("✓ {} completed", self.name())]
        }
    }
}

// ── Registration via inventory ─────────────────────────────────────

pub struct ToolDisplayEntry {
    pub name: &'static str,
    pub display: fn() -> Box<dyn ToolDisplay>,
}

inventory::collect!(ToolDisplayEntry);

static TOOL_DISPLAYS: LazyLock<HashMap<&'static str, Box<dyn ToolDisplay>>> = LazyLock::new(|| {
    let mut map: HashMap<&'static str, Box<dyn ToolDisplay>> = HashMap::new();
    for entry in inventory::iter::<ToolDisplayEntry> {
        map.insert(entry.name, (entry.display)());
    }
    map
});

fn lookup_display(name: &str) -> Option<&dyn ToolDisplay> {
    TOOL_DISPLAYS.get(name).map(|display| display.as_ref())
}

/// Format a tool call for human-friendly display.
pub fn format_tool_call(name: &str, raw_json: &str) -> (String, Vec<String>) {
    let parsed: serde_json::Value =
        serde_json::from_str(raw_json).unwrap_or(serde_json::Value::Null);

    if name == "TodoWrite" {
        return format_todowrite_value(&parsed).unwrap_or_else(|| {
            let truncated = truncate_json(raw_json);
            (format!("● {name}"), vec![truncated])
        });
    }

    if name == "TodoRun" {
        return (
            "● TodoRun".to_string(),
            vec!["execute all pending todos".to_string()],
        );
    }

    if let Some(display) = lookup_display(name) {
        return (
            display.format_header(&parsed),
            display.format_details(&parsed),
        );
    }

    // Fallback for unknown tools
    let truncated = truncate_json(raw_json);
    (format!("● {name}"), vec![truncated])
}

fn debug_log(msg: &str) {
    log::debug!("{}", msg);
}

impl super::OutputArea {
    /// 流式过程中 tool_use_start 时推送预占 header，立刻让用户看到 tool 被调用。
    /// `index` 是 LLM 返回的 tool call index，用于生成唯一 pending id
    /// （如 pending:Agent:2），以便后续精确匹配和原地更新。
    pub fn push_tool_call_start(&mut self, name: &str, index: usize) {
        self.finish_streaming();
        self.push_line(OutputLine {
            content: format!("● {name}..."),
            style: LineStyle::ToolCallRunning,
            tool_id: Some(format!("pending:{name}:{index}")),
        });
    }

    /// 流式 arguments delta 到达时更新 pending 占位行内容。
    /// 尝试从 partial JSON 中提取关键参数并更新显示。
    /// 使用 `name` + `index` 精确匹配 pending 行。
    pub fn update_tool_call_pending(&mut self, name: &str, index: usize, partial_args: &str) {
        let preview = common::extract_tool_preview(name, partial_args);
        if preview.is_empty() {
            return;
        }
        let pending_id = format!("pending:{name}:{index}");
        let new_content = format!("● {name}({preview})");
        for line in &mut self.lines {
            if line.tool_id.as_deref() == Some(&pending_id) {
                line.content = new_content;
                break;
            }
        }
    }

    /// 更新 Agent 工具调用的进度显示（原地替换 pending 占位行）
    pub fn push_tool_call(&mut self, tool_id: &str, name: &str, summary: &str) {
        self.finish_streaming();

        let (header, details) = if name == "TodoWrite" {
            self.format_todowrite(summary)
        } else {
            format_tool_call(name, summary)
        };

        let detail_style = lookup_display(name)
            .map(|display| display.detail_style())
            .unwrap_or(LineStyle::System);

        // 查找第一个匹配的 pending 占位行，原地替换
        let prefix = format!("pending:{name}:");
        if let Some(pos) = self.lines.iter().position(|line| {
            line.tool_id
                .as_deref()
                .is_some_and(|id| id.starts_with(&prefix))
        }) {
            // 原地更新 header 行
            self.lines[pos].content = header;
            self.lines[pos].style = LineStyle::ToolCallRunning;
            self.lines[pos].tool_id = Some(tool_id.to_string());

            // 在 header 后插入 detail 行
            let detail_lines: Vec<OutputLine> = details
                .iter()
                .map(|detail| OutputLine {
                    content: format!("{INDENT}{detail}"),
                    style: detail_style,
                    tool_id: Some(tool_id.to_string()),
                })
                .collect();

            for (i, dl) in detail_lines.into_iter().enumerate() {
                self.lines.insert(pos + 1 + i, dl);
            }
            return;
        }

        // 没有 pending 占位行（非流式路径），直接追加
        self.push_line(OutputLine {
            content: header,
            style: LineStyle::ToolCallRunning,
            tool_id: Some(tool_id.to_string()),
        });

        for detail in details.iter() {
            self.push_line(OutputLine {
                content: format!("{INDENT}{detail}"),
                style: detail_style,
                tool_id: Some(tool_id.to_string()),
            });
        }
    }

    fn format_todowrite(&mut self, raw_json: &str) -> (String, Vec<String>) {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(raw_json);
        debug_log(&format!("TodoWrite raw_json: {raw_json}"));

        if let Ok(value) = parsed {
            if let Some(todos) = value.get("todos").and_then(|todos| todos.as_array()) {
                for todo in todos.iter() {
                    if let (Some(id), Some(subject)) = (
                        todo.get("id").and_then(|field| field.as_str()),
                        todo.get("subject").and_then(|field| field.as_str()),
                    ) {
                        self.todo_subject_cache
                            .insert(id.to_string(), subject.to_string());
                    }
                }

                if let Some((header, details)) = format_todowrite_value(&value) {
                    return (header, details);
                }
            }
        }

        format_tool_call("TodoWrite", raw_json)
    }
}

#[cfg(test)]
#[path = "tool_display_agent_tests.rs"]
mod agent_tests;

#[cfg(test)]
mod tests {
    use super::super::OutputArea;

    #[test]
    fn test_task_list_create_display_hides_success_result_noise() {
        let mut output = OutputArea::new();

        output.push_tool_call(
            "task-list-create-1",
            "TaskListCreate",
            r#"{"subject":"修复任务","summary":"处理当前请求"}"#,
        );
        output.push_tool_result_with_diff(
            "task-list-create-1",
            "TaskListCreate",
            "Task list #1 created\nSummary: 处理当前请求",
            false,
            "",
        );

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref() == Some("task-list-create-1"))
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            matching,
            vec!["✓ TaskListCreate: 修复任务", "  处理当前请求", ""]
        );
    }

    #[test]
    fn test_task_list_complete_display_shows_only_success_header() {
        let mut output = OutputArea::new();

        output.push_tool_call("task-list-complete-1", "TaskListComplete", "{}");
        output.push_tool_result_with_diff(
            "task-list-complete-1",
            "TaskListComplete",
            "Task list #1 completed",
            false,
            "",
        );

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref() == Some("task-list-complete-1"))
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(matching, vec!["✓ TaskListComplete"]);
    }
}

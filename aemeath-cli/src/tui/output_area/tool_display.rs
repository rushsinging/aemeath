use std::collections::HashMap;
use std::sync::LazyLock;

use crate::tui::output_area::{LineStyle, OutputLine, INDENT};

mod agent;
mod common;
mod results;
mod task_impls;
mod tool_impls;

use common::{format_todowrite_value, truncate_json};

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

    /// Max lines of result output to show (default 3).
    fn result_max_lines(&self) -> usize {
        10
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
    /// 流式过程中 tool_use_start 时推送预占 header，立刻让用户看到 tool 被调用
    pub fn push_tool_call_start(&mut self, name: &str) {
        self.finish_streaming();
        self.push_line(OutputLine {
            content: format!("● {name}..."),
            style: LineStyle::ToolCallRunning,
            tool_id: Some(format!("pending:{name}")),
        });
    }

    /// 更新 Agent 工具调用的进度显示（实时替换 header 行文本）
    pub fn push_tool_call(&mut self, tool_id: &str, name: &str, summary: &str) {
        self.finish_streaming();

        // 清除该 tool 的预占 header（如果有）
        let pending_id = format!("pending:{name}");
        if let Some(pos) = self
            .lines
            .iter()
            .position(|line| line.tool_id.as_deref() == Some(&pending_id))
        {
            self.lines.remove(pos);
        }

        let (header, details) = if name == "TodoWrite" {
            self.format_todowrite(summary)
        } else {
            format_tool_call(name, summary)
        };

        self.push_line(OutputLine {
            content: header,
            style: LineStyle::ToolCallRunning,
            tool_id: Some(tool_id.to_string()),
        });

        let detail_style = lookup_display(name)
            .map(|display| display.detail_style())
            .unwrap_or(LineStyle::System);
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
mod tests {
    use super::super::OutputArea;
    use aemeath_core::tool::{AgentProgressEvent, AgentProgressKind, AgentToolCallProgress};

    #[test]
    fn test_push_agent_progress_replaces_tool_calls_for_same_agent() {
        let mut output = OutputArea::new();

        output.push_agent_progress(
            "agent-1",
            tool_calls_event(1, vec![call("1", "Read", "old.rs")]),
        );
        output.push_agent_progress(
            "agent-1",
            tool_calls_event(
                2,
                vec![
                    call("2", "Read", "new.rs"),
                    call("3", "Grep", "\"needle\" in src"),
                ],
            ),
        );

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref() == Some("agent-1"))
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(matching, vec!["  ↳ Read: new.rs | Grep: \"needle\" in src"]);
    }

    #[test]
    fn test_push_agent_progress_keeps_different_agent_tool_calls_separate() {
        let mut output = OutputArea::new();

        output.push_agent_progress(
            "agent-1",
            tool_calls_event(1, vec![call("1", "Read", "a.rs")]),
        );
        output.push_agent_progress(
            "agent-2",
            tool_calls_event(1, vec![call("2", "Bash", "cargo check")]),
        );

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref().is_some())
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(matching, vec!["  ↳ Read: a.rs", "  ↳ Bash: cargo check"]);
    }

    #[test]
    fn test_push_agent_progress_groups_duplicate_tools_without_showing_turn() {
        let mut output = OutputArea::new();

        output.push_agent_progress(
            "agent-1",
            tool_calls_event(
                7,
                vec![
                    call("1", "Read", "a.rs"),
                    call("2", "Read", "b.rs"),
                    call("3", "Read", "c.rs"),
                    call("4", "Read", "d.rs"),
                ],
            ),
        );

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref() == Some("agent-1"))
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(matching, vec!["  ↳ Read ×4: a.rs, b.rs, c.rs +1 more"]);
    }

    #[test]
    fn test_push_agent_progress_appends_message_events() {
        let mut output = OutputArea::new();

        output.push_agent_progress("agent-1", message_event(1, "plain progress"));
        output.push_agent_progress("agent-1", message_event(2, "another progress"));

        let matching = output
            .lines
            .iter()
            .filter(|line| line.tool_id.as_deref() == Some("agent-1"))
            .map(|line| line.content.as_str())
            .collect::<Vec<_>>();

        assert_eq!(matching, vec!["  ↳ plain progress", "  ↳ another progress"]);
    }

    fn tool_calls_event(sequence: usize, calls: Vec<AgentToolCallProgress>) -> AgentProgressEvent {
        AgentProgressEvent {
            sequence,
            kind: AgentProgressKind::ToolCalls { calls },
        }
    }

    fn message_event(sequence: usize, text: &str) -> AgentProgressEvent {
        AgentProgressEvent {
            sequence,
            kind: AgentProgressKind::Message {
                text: text.to_string(),
            },
        }
    }

    fn call(id: &str, name: &str, summary: &str) -> AgentToolCallProgress {
        AgentToolCallProgress {
            id: id.to_string(),
            name: name.to_string(),
            input: serde_json::json!({}),
            summary: summary.to_string(),
        }
    }
}

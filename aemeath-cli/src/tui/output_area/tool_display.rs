use std::collections::HashMap;
use std::sync::LazyLock;

use aemeath_core::tool::{AgentProgressEvent, AgentProgressKind, AgentToolCallProgress};

use crate::tui::output_area::{build_diff_lines, display, LineStyle, OutputLine, INDENT};
use crate::tui::safe_text;

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
    TOOL_DISPLAYS.get(name).map(|b| b.as_ref())
}

// ── Per-tool implementations ───────────────────────────────────────

struct BashDisplay;
impl ToolDisplay for BashDisplay {
    fn name(&self) -> &str {
        "Bash"
    }
    fn format_header(&self, _input: &serde_json::Value) -> String {
        "● Bash".to_string()
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let cmd = input.get("command").and_then(|c| c.as_str()).unwrap_or("?");
        let timeout = input.get("timeout").and_then(|t| t.as_u64());
        let max_cmd_width = 120usize.saturating_sub(INDENT.len() + 2);
        let truncated = display::truncate_unicode_width(cmd, max_cmd_width);
        let mut detail = format!("$ {truncated}");
        if let Some(t) = timeout {
            if t != 120_000 {
                detail.push_str(&format!("  (timeout: {}s)", t / 1000));
            }
        }
        vec![detail]
    }
    fn detail_style(&self) -> LineStyle {
        LineStyle::Normal
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Bash",
    display: || Box::new(BashDisplay)
});

struct ReadDisplay;
impl ToolDisplay for ReadDisplay {
    fn name(&self) -> &str {
        "Read"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let path = input
            .get("file_path")
            .and_then(|p| p.as_str())
            .unwrap_or("?");
        format!("● Read({path})")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let path = input
            .get("file_path")
            .and_then(|p| p.as_str())
            .unwrap_or("?");
        let offset = input.get("offset").and_then(|o| o.as_u64());
        let limit = input.get("limit").and_then(|l| l.as_u64());
        let mut detail = format!("Read {path}");
        if let Some(o) = offset {
            detail.push_str(&format!(" (offset: {o}"));
            if let Some(l) = limit {
                detail.push_str(&format!(", limit: {l}"));
            }
            detail.push(')');
        }
        vec![detail]
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Read",
    display: || Box::new(ReadDisplay)
});

struct WriteDisplay;
impl ToolDisplay for WriteDisplay {
    fn name(&self) -> &str {
        "Write"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let path = input
            .get("file_path")
            .and_then(|p| p.as_str())
            .unwrap_or("?");
        format!("● Write({path})")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let content = input.get("content").and_then(|c| c.as_str()).unwrap_or("");
        vec![format!("{} bytes", content.len())]
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Write",
    display: || Box::new(WriteDisplay)
});

struct EditDisplay;
impl ToolDisplay for EditDisplay {
    fn name(&self) -> &str {
        "Edit"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let path = input
            .get("file_path")
            .and_then(|p| p.as_str())
            .unwrap_or("?");
        format!("● Edit({path})")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let old = input
            .get("old_string")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let new = input
            .get("new_string")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let old_lines = old.lines().count();
        let new_lines = new.lines().count();
        let detail = if old_lines == new_lines {
            format!("Changed {} -> {} chars", old.len(), new.len())
        } else if new_lines > old_lines {
            format!(
                "Added {} line(s), {} -> {} chars",
                new_lines - old_lines,
                old.len(),
                new.len()
            )
        } else {
            format!(
                "Removed {} line(s), {} -> {} chars",
                old_lines - new_lines,
                old.len(),
                new.len()
            )
        };
        vec![detail]
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Edit",
    display: || Box::new(EditDisplay)
});

struct GlobDisplay;
impl ToolDisplay for GlobDisplay {
    fn name(&self) -> &str {
        "Glob"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let pattern = input.get("pattern").and_then(|p| p.as_str()).unwrap_or("?");
        format!("● Glob({pattern})")
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Glob",
    display: || Box::new(GlobDisplay)
});

struct GrepDisplay;
impl ToolDisplay for GrepDisplay {
    fn name(&self) -> &str {
        "Grep"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let pattern = input.get("pattern").and_then(|p| p.as_str()).unwrap_or("?");
        format!("● Grep /{pattern}/")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let path = input.get("path").and_then(|p| p.as_str()).unwrap_or(".");
        vec![format!("in {path}")]
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Grep",
    display: || Box::new(GrepDisplay)
});

struct AgentDisplay;
impl ToolDisplay for AgentDisplay {
    fn name(&self) -> &str {
        "Agent"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let desc = input
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("sub-task");
        let role = input.get("role").and_then(|r| r.as_str());
        let model = input.get("model").and_then(|m| m.as_str());
        let mut header = format!("● Agent({desc})");
        if let Some(r) = role {
            header.push_str(&format!("  [role: {r}]"));
        }
        if let Some(m) = model {
            header.push_str(&format!("  [model: {m}]"));
        }
        header
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let prompt = input.get("prompt").and_then(|p| p.as_str()).unwrap_or("");
        if prompt.is_empty() {
            return vec![];
        }
        let max_prompt = 200usize.saturating_sub(INDENT.len());
        let preview = if prompt.len() > max_prompt {
            let (prefix, _) = safe_text::truncate_unicode_width(prompt, max_prompt);
            format!("{}...", prefix)
        } else {
            prompt.to_string()
        };
        vec![preview]
    }
    fn result_max_lines(&self) -> usize {
        20
    }
    fn result_style(&self) -> LineStyle {
        LineStyle::Assistant
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Agent",
    display: || Box::new(AgentDisplay)
});

struct WebFetchDisplay;
impl ToolDisplay for WebFetchDisplay {
    fn name(&self) -> &str {
        "WebFetch"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let url = input.get("url").and_then(|u| u.as_str()).unwrap_or("?");
        format!("● WebFetch({url})")
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "WebFetch",
    display: || Box::new(WebFetchDisplay)
});

struct TaskCreateDisplay;
impl ToolDisplay for TaskCreateDisplay {
    fn name(&self) -> &str {
        "TaskCreate"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let subject = input.get("subject").and_then(|s| s.as_str()).unwrap_or("?");
        format!("● TaskCreate({subject})")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let desc = input
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("");
        if desc.is_empty() {
            return vec![];
        }
        let max = 80usize;
        let preview = if desc.len() > max {
            let (prefix, _) = safe_text::truncate_unicode_width(desc, max);
            format!("{}...", prefix)
        } else {
            desc.to_string()
        };
        vec![preview]
    }
    fn result_max_lines(&self) -> usize {
        20
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskCreate",
    display: || Box::new(TaskCreateDisplay)
});

struct TaskUpdateDisplay;
impl ToolDisplay for TaskUpdateDisplay {
    fn name(&self) -> &str {
        "TaskUpdate"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let id = input.get("taskId").and_then(|s| s.as_str()).unwrap_or("?");
        format!("● TaskUpdate({id})")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let status = input.get("status").and_then(|s| s.as_str()).unwrap_or("");
        if status.is_empty() {
            return vec![];
        }
        vec![format!("-> {status}")]
    }
    fn result_max_lines(&self) -> usize {
        20
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskUpdate",
    display: || Box::new(TaskUpdateDisplay)
});

struct TaskListDisplay;
impl ToolDisplay for TaskListDisplay {
    fn name(&self) -> &str {
        "TaskList"
    }
    fn format_header(&self, _input: &serde_json::Value) -> String {
        "● TaskList".to_string()
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn result_max_lines(&self) -> usize {
        20
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskList",
    display: || Box::new(TaskListDisplay)
});

struct SkillDisplay;
impl ToolDisplay for SkillDisplay {
    fn name(&self) -> &str {
        "Skill"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let skill = input.get("skill").and_then(|s| s.as_str()).unwrap_or("?");
        format!("● Skill({skill})")
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "Skill",
    display: || Box::new(SkillDisplay)
});

struct LspDisplay;
impl ToolDisplay for LspDisplay {
    fn name(&self) -> &str {
        "LSP"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let op = input
            .get("operation")
            .and_then(|o| o.as_str())
            .unwrap_or("?");
        let path = input
            .get("filePath")
            .and_then(|p| p.as_str())
            .unwrap_or("?");
        format!("● LSP::{op}({path})")
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "LSP",
    display: || Box::new(LspDisplay)
});

struct TaskGetDisplay;
impl ToolDisplay for TaskGetDisplay {
    fn name(&self) -> &str {
        "TaskGet"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let id = input.get("taskId").and_then(|s| s.as_str()).unwrap_or("?");
        format!("● TaskGet({id})")
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskGet",
    display: || Box::new(TaskGetDisplay)
});

struct TaskStopDisplay;
impl ToolDisplay for TaskStopDisplay {
    fn name(&self) -> &str {
        "TaskStop"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let id = input.get("taskId").and_then(|s| s.as_str()).unwrap_or("?");
        format!("● TaskStop({id})")
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn detail_style(&self) -> LineStyle {
        LineStyle::ToolCallError
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskStop",
    display: || Box::new(TaskStopDisplay)
});

struct TaskOutputDisplay;
impl ToolDisplay for TaskOutputDisplay {
    fn name(&self) -> &str {
        "TaskOutput"
    }
    fn format_header(&self, _input: &serde_json::Value) -> String {
        "● TaskOutput".to_string()
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskOutput",
    display: || Box::new(TaskOutputDisplay)
});

struct EnterPlanModeDisplay;
impl ToolDisplay for EnterPlanModeDisplay {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let reason = input.get("reason").and_then(|r| r.as_str()).unwrap_or("");
        if reason.is_empty() {
            "📋 Enter Plan Mode".to_string()
        } else {
            format!("📋 Plan: {reason}")
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec!["Tool calls will be simulated, not executed.".to_string()]
    }
    fn result_max_lines(&self) -> usize {
        0
    }
    fn format_result_summary(&self, _result: &str, is_error: bool) -> Vec<String> {
        if is_error {
            vec!["✗ Failed to enter plan mode".to_string()]
        } else {
            vec![]
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "EnterPlanMode",
    display: || Box::new(EnterPlanModeDisplay)
});

struct ExitPlanModeDisplay;
impl ToolDisplay for ExitPlanModeDisplay {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let execute = input
            .get("execute")
            .and_then(|e| e.as_bool())
            .unwrap_or(false);
        if execute {
            "▶ Execute Plan".to_string()
        } else {
            "▶ Exit Plan Mode".to_string()
        }
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let execute = input
            .get("execute")
            .and_then(|e| e.as_bool())
            .unwrap_or(false);
        if execute {
            vec!["Planned actions will now be executed.".to_string()]
        } else {
            vec!["Returning to normal execution.".to_string()]
        }
    }
    fn result_max_lines(&self) -> usize {
        0
    }
    fn format_result_summary(&self, _result: &str, is_error: bool) -> Vec<String> {
        if is_error {
            vec!["✗ Failed to exit plan mode".to_string()]
        } else {
            vec![]
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "ExitPlanMode",
    display: || Box::new(ExitPlanModeDisplay)
});

fn debug_log(msg: &str) {
    use std::io::Write;
    let path = dirs::home_dir()
        .unwrap_or_default()
        .join(".aemeath")
        .join("debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

/// Format a tool call for human-friendly display.
pub fn format_tool_call(name: &str, raw_json: &str) -> (String, Vec<String>) {
    let parsed: serde_json::Value =
        serde_json::from_str(raw_json).unwrap_or(serde_json::Value::Null);

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

fn truncate_json(raw: &str) -> String {
    if raw.len() > 100 {
        let (prefix, _) = safe_text::truncate_unicode_width(raw, 100);
        format!("{}...", prefix)
    } else {
        raw.to_string()
    }
}

fn format_agent_tool_calls(calls: &[AgentToolCallProgress]) -> String {
    let mut grouped: Vec<(&str, Vec<&str>)> = Vec::new();
    for call in calls {
        if let Some((_, summaries)) = grouped.iter_mut().find(|(name, _)| *name == call.name) {
            summaries.push(call.summary.as_str());
        } else {
            grouped.push((call.name.as_str(), vec![call.summary.as_str()]));
        }
    }

    grouped
        .into_iter()
        .map(|(name, summaries)| {
            let count = summaries.len();
            let visible = summaries
                .iter()
                .filter(|summary| !summary.is_empty())
                .take(3)
                .copied()
                .collect::<Vec<_>>();
            let suffix = if visible.is_empty() {
                String::new()
            } else {
                let mut text = visible.join(", ");
                if count > 3 {
                    text.push_str(&format!(" +{} more", count - 3));
                }
                format!(": {text}")
            };
            if count > 1 {
                format!("{name} ×{count}{suffix}")
            } else {
                format!("{name}{suffix}")
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
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
            .position(|l| l.tool_id.as_deref() == Some(&pending_id))
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
            .map(|d| d.detail_style())
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

        if let Ok(v) = parsed {
            if let Some(todos) = v.get("todos").and_then(|t| t.as_array()) {
                let count = todos.len();
                let mut details: Vec<String> = Vec::new();

                for todo in todos.iter() {
                    if let (Some(id), Some(subject)) = (
                        todo.get("id").and_then(|s| s.as_str()),
                        todo.get("subject").and_then(|s| s.as_str()),
                    ) {
                        self.todo_subject_cache
                            .insert(id.to_string(), subject.to_string());
                    }
                }

                for todo in todos.iter().take(3) {
                    let subject = todo
                        .get("subject")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| {
                            todo.get("id")
                                .and_then(|s| s.as_str())
                                .and_then(|id| self.todo_subject_cache.get(id).cloned())
                        })
                        .unwrap_or_else(|| "?".to_string());

                    let status = todo
                        .get("status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("pending");
                    let icon = match status {
                        "completed" => "✓",
                        "in_progress" => "◐",
                        _ => "○",
                    };
                    details.push(format!("{icon} {subject}"));
                }
                if count > 3 {
                    details.push(format!("... +{} more", count - 3));
                }
                return (format!("● TodoWrite ({count} items)"), details);
            }
        }

        format_tool_call("TodoWrite", raw_json)
    }

    pub fn push_agent_progress(&mut self, tool_id: &str, event: AgentProgressEvent) {
        match event.kind {
            AgentProgressKind::ToolCalls { calls } => {
                self.finish_streaming();
                let summary = format_agent_tool_calls(&calls);
                let content = format!("{INDENT}↳ {summary}");
                if let Some(line) = self.lines.iter_mut().rev().find(|line| {
                    line.tool_id.as_deref() == Some(tool_id)
                        && line.content.starts_with(&format!("{INDENT}↳ "))
                }) {
                    line.content = content;
                    line.style = LineStyle::System;
                    return;
                }

                let progress_line = OutputLine {
                    content,
                    style: LineStyle::System,
                    tool_id: Some(tool_id.to_string()),
                };
                let insert_at = self
                    .lines
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, line)| line.tool_id.as_deref() == Some(tool_id))
                    .map(|(idx, _)| idx + 1)
                    .unwrap_or(self.lines.len());
                self.insert_lines_at(insert_at, vec![progress_line]);
            }
            AgentProgressKind::Message { text } => {
                self.push_tool_progress(tool_id, &text);
            }
        }
    }

    pub fn push_tool_progress(&mut self, tool_id: &str, text: &str) {
        self.finish_streaming();

        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        let content = format!("{INDENT}↳ {trimmed}");
        let already_shown = self
            .lines
            .iter()
            .rev()
            .take(8)
            .any(|line| line.tool_id.as_deref() == Some(tool_id) && line.content == content);
        if already_shown {
            return;
        }

        let id_tag = Some(tool_id.to_string());
        let progress_line = OutputLine {
            content,
            style: LineStyle::System,
            tool_id: id_tag,
        };

        let insert_at = self
            .lines
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line)| line.tool_id.as_deref() == Some(tool_id))
            .map(|(idx, _)| idx + 1)
            .unwrap_or(self.lines.len());

        self.insert_lines_at(insert_at, vec![progress_line]);
    }

    pub fn push_tool_result_with_diff(
        &mut self,
        tool_id: &str,
        tool_name: &str,
        result: &str,
        is_error: bool,
        image_note: &str,
    ) {
        self.finish_streaming();

        let done_icon = if is_error { "✗" } else { "✓" };
        let done_style = if is_error {
            LineStyle::ToolCallError
        } else {
            LineStyle::ToolCallSuccess
        };

        let mut header_idx: Option<usize> = None;
        for (idx, line) in self.lines.iter_mut().enumerate() {
            if matches!(line.style, LineStyle::ToolCallRunning)
                && line.tool_id.as_deref() == Some(tool_id)
            {
                line.content = line.content.replacen('●', done_icon, 1);
                line.style = done_style;
                header_idx = Some(idx);
                break;
            }
        }
        if header_idx.is_none() {
            for (idx, line) in self.lines.iter_mut().enumerate().rev() {
                if matches!(line.style, LineStyle::ToolCallRunning) {
                    line.content = line.content.replacen('●', done_icon, 1);
                    line.style = done_style;
                    header_idx = Some(idx);
                    break;
                }
            }
        }

        let id_tag = Some(tool_id.to_string());
        let mut result_lines: Vec<OutputLine> = Vec::new();

        if is_error {
            result_lines.push(OutputLine {
                content: format!("{INDENT}✗ {result}"),
                style: LineStyle::ToolCallError,
                tool_id: id_tag.clone(),
            });
        } else if tool_name == "Edit" && result.contains("---DIFF---\n") {
            let parts: Vec<&str> = result.splitn(3, "---DIFF---\n").collect();
            if parts.len() == 3 {
                let summary = parts[0].trim();
                build_diff_lines(parts[1], parts[2], &id_tag, &mut result_lines);
                result_lines.push(OutputLine {
                    content: format!("{INDENT}✓ {summary}"),
                    style: LineStyle::ToolCallSuccess,
                    tool_id: id_tag.clone(),
                });
            } else {
                let summaries = lookup_display(tool_name)
                    .map(|d| d.format_result_summary(result, is_error))
                    .unwrap_or_else(|| vec![format!("{INDENT}✓ {tool_name} completed")]);
                for s in summaries {
                    result_lines.push(OutputLine {
                        content: format!("{INDENT}{s}"),
                        style: LineStyle::ToolCallSuccess,
                        tool_id: id_tag.clone(),
                    });
                }
            }
        } else {
            if !result.trim().is_empty() {
                let (max_lines, result_style) = lookup_display(tool_name)
                    .map(|d| (d.result_max_lines(), d.result_style()))
                    .unwrap_or((3, LineStyle::System));

                let total = result.lines().count();
                for line in result.lines().take(max_lines) {
                    result_lines.push(OutputLine {
                        content: format!("{INDENT}{line}"),
                        style: result_style,
                        tool_id: id_tag.clone(),
                    });
                }
                if total > max_lines {
                    result_lines.push(OutputLine {
                        content: format!("{INDENT}... ({} lines omitted)", total - max_lines),
                        style: result_style,
                        tool_id: id_tag.clone(),
                    });
                }
            }
            let summaries = lookup_display(tool_name)
                .map(|d| d.format_result_summary(result, is_error))
                .unwrap_or_else(|| vec![format!("✓ {tool_name} completed")]);
            for s in summaries {
                result_lines.push(OutputLine {
                    content: format!("{INDENT}{s}"),
                    style: if is_error {
                        LineStyle::ToolCallError
                    } else {
                        LineStyle::ToolCallSuccess
                    },
                    tool_id: id_tag.clone(),
                });
            }
        }

        if !image_note.is_empty() {
            result_lines.push(OutputLine {
                content: image_note.trim().to_string(),
                style: LineStyle::System,
                tool_id: id_tag.clone(),
            });
        }

        result_lines.push(OutputLine {
            content: String::new(),
            style: LineStyle::System,
            tool_id: id_tag.clone(),
        });

        let insert_at = if let Some(start) = header_idx {
            let mut end = start;
            while end + 1 < self.lines.len()
                && self.lines[end + 1].tool_id.as_deref() == Some(tool_id)
            {
                end += 1;
            }
            end + 1
        } else {
            self.lines.len()
        };

        self.insert_lines_at(insert_at, result_lines);
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

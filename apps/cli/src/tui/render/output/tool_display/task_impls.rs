use super::common::{truncate_ellipsis, typed_data};
use super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use sdk::tool_input::{
    TaskBlockByInput, TaskCreateInput, TaskGetInput, TaskListCreateInput, TaskStopInput,
    TaskUpdateInput,
};
use sdk::tool_result::TaskUpdateResult;
use std::path::Path;

/// Deserialize a typed Input from a raw `serde_json::Value`, tolerating
/// missing / malformed fields via `Default`.  serde `#[serde(alias = ...)]`
/// attributes are honoured, eliminating the snake_case / camelCase mismatch
/// that `str_arg` caused (issue #839).
fn parse_input<T: serde::de::DeserializeOwned + Default>(input: &serde_json::Value) -> T {
    serde_json::from_value(input.clone()).unwrap_or_default()
}

fn json_str<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .unwrap_or("-")
}

fn format_task_view_line(task: &serde_json::Value) -> String {
    let id = json_str(task, "id");
    let status = json_str(task, "status");
    let subject = truncate_ellipsis(json_str(task, "subject"), 60);
    format!("{id} · {status} · {subject}")
}

fn format_task_list_summary_line(summary: &serde_json::Value) -> Option<String> {
    let task_list = summary.get("task_list")?;
    // task_list: null（无当前列表）不得回退成 `- · - · - · pending:0…`。
    if !task_list.is_object() {
        return None;
    }
    let stats = summary.get("stats");
    let id = json_str(task_list, "id");
    let status = json_str(task_list, "status");
    let list_summary = truncate_ellipsis(json_str(task_list, "summary"), 50);
    let stats_part = stats
        .map(|value| {
            format!(
                "pending:{} doing:{} completed:{} total:{}",
                value.get("pending").and_then(|v| v.as_u64()).unwrap_or(0),
                value
                    .get("in_progress")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                value.get("completed").and_then(|v| v.as_u64()).unwrap_or(0),
                value.get("total").and_then(|v| v.as_u64()).unwrap_or(0),
            )
        })
        .unwrap_or_else(|| "-".to_string());
    Some(format!("{id} · {status} · {list_summary} · {stats_part}"))
}

// ── TaskCreate ───────────────────────────────────────────────────

struct TaskCreateDisplay;
impl ToolDisplay for TaskCreateDisplay {
    fn name(&self) -> &str {
        "TaskCreate"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<TaskCreateInput>(input);
        if args.subject.is_empty() {
            return self.display_name().to_string();
        }
        if args.description.is_empty() {
            format!("{} {}", args.subject, self.display_name())
        } else {
            format!(
                "{} {}: {}",
                self.display_name(),
                args.subject,
                truncate_ellipsis(&args.description, 60)
            )
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Compact,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Hidden,
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskCreate",
    display: || Box::new(TaskCreateDisplay)
});

// ── TaskUpdate ───────────────────────────────────────────────────

struct TaskUpdateDisplay;
impl ToolDisplay for TaskUpdateDisplay {
    fn name(&self) -> &str {
        "TaskUpdate"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<TaskUpdateInput>(input);
        if args.task_id.is_empty() {
            return self.display_name().to_string();
        }
        let summary = self.header_summary(input, None);
        match summary.is_empty() {
            false => format!("{} {} — {}", self.display_name(), args.task_id, summary),
            true => format!("{} {}", self.display_name(), args.task_id),
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Compact,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Hidden,
        }
    }
    /// result 到达后从 typed payload 取 subject 回填 header（issue #486）。
    /// LLM 调用 TaskUpdate 时通常只传 task_id + status，subject 在 TaskCreate
    /// 时设定，只有 store 回填的 result 才有 → 故必须覆写此方法。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let args = parse_input::<TaskUpdateInput>(input);
        if args.task_id.is_empty() {
            return self.format_header_line(input, workspace_root);
        }
        let summary = self.header_summary(input, result_payload);
        let name = self.display_name().to_string();
        let mut spans = vec![
            Span::styled(name, Style::default().fg(theme::ACCENT_BRIGHT)),
            Span::raw(format!(" {}", args.task_id)),
        ];
        if !summary.is_empty() {
            spans.push(Span::raw(format!(" — {summary}")));
        }
        Line::from(spans)
    }
}
impl TaskUpdateDisplay {
    /// 构建 header 摘要片段（subject 紧跟 id，其余按重要性排序）。
    /// `result_payload` 非空时优先从 typed result 取 subject（store 回填）。
    /// key-value 模式：从 `key` + `value` 提取变更摘要。
    ///
    /// 片段间以单空格连接，NEVER 逗号：`修复渲染 bug → in_progress` /
    /// `修复渲染 bug p=high`。TaskBlockBy 的依赖列表枚举（`blocked by #4, #5`）
    /// 属列表分隔符语义，不在此列。
    fn header_summary(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
    ) -> String {
        let args = parse_input::<TaskUpdateInput>(input);

        // subject 优先从 typed result 取（store 回填）
        let typed: Option<TaskUpdateResult> = typed_data(result_payload);
        let subject = typed
            .as_ref()
            .map(|r| r.subject.as_str())
            .filter(|s: &&str| !s.is_empty())
            .unwrap_or("");

        let mut parts = Vec::new();
        if !subject.is_empty() {
            parts.push(truncate_ellipsis(subject, 40));
        }
        match args.key.as_str() {
            "status" => {
                if let Some(s) = args.value.as_str() {
                    parts.push(format!("→ {s}"));
                }
            }
            "priority" => {
                if let Some(s) = args.value.as_str() {
                    parts.push(format!("p={s}"));
                }
            }
            "subject" | "description" | "owner" => {
                // 这些字段变更不额外展示在 header，subject 从 result 回填即可
            }
            _ => {}
        }
        parts.join(" ")
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskUpdate",
    display: || Box::new(TaskUpdateDisplay)
});

// ── TaskBlockBy ──────────────────────────────────────────────────

struct TaskBlockByDisplay;
impl ToolDisplay for TaskBlockByDisplay {
    fn name(&self) -> &str {
        "TaskBlockBy"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<TaskBlockByInput>(input);
        if args.id.is_empty() {
            return self.display_name().to_string();
        }
        let dependencies = args
            .block_by_ids
            .iter()
            .map(|id| format!("#{id}"))
            .collect::<Vec<_>>();
        if dependencies.is_empty() {
            format!("{} #{} — clear blockers", self.display_name(), args.id)
        } else {
            format!(
                "{} #{} — blocked by {}",
                self.display_name(),
                args.id,
                dependencies.join(", ")
            )
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Compact,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Hidden,
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskBlockBy",
    display: || Box::new(TaskBlockByDisplay)
});

// ── TaskListGet ──────────────────────────────────────────────────

struct TaskListGetDisplay;
impl ToolDisplay for TaskListGetDisplay {
    fn name(&self) -> &str {
        "TaskListGet"
    }
    fn format_header(&self, _input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        self.display_name().to_string()
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let content = content?;
        let mut lines = Vec::new();
        if let Some(summary_line) = content.get("task_list").and_then(|task_list| {
            let wrapper = serde_json::json!({
                "task_list": task_list,
                "stats": content.get("stats").cloned().unwrap_or(serde_json::Value::Null),
            });
            format_task_list_summary_line(&wrapper)
        }) {
            lines.push(summary_line);
        }
        if let Some(tasks) = content.get("tasks").and_then(|value| value.as_array()) {
            if tasks.is_empty() && lines.is_empty() {
                return Some(vec!["（无任务）".to_string()]);
            }
            lines.extend(tasks.iter().map(format_task_view_line));
        } else if lines.is_empty() {
            return Some(vec!["（无列表）".to_string()]);
        }
        Some(lines)
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(15),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskListGet",
    display: || Box::new(TaskListGetDisplay)
});

// ── TaskLists ────────────────────────────────────────────────────

struct TaskListsDisplay;
impl ToolDisplay for TaskListsDisplay {
    fn name(&self) -> &str {
        "TaskLists"
    }
    fn format_header(&self, _input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        self.display_name().to_string()
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let lists = content?
            .get("task_lists")
            .and_then(|value| value.as_array())?;
        if lists.is_empty() {
            return Some(vec!["（无列表）".to_string()]);
        }
        Some(
            lists
                .iter()
                .filter_map(format_task_list_summary_line)
                .collect(),
        )
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(15),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskLists",
    display: || Box::new(TaskListsDisplay)
});

// ── TaskListCreate ───────────────────────────────────────────────

struct TaskListCreateDisplay;
impl ToolDisplay for TaskListCreateDisplay {
    fn name(&self) -> &str {
        "TaskListCreate"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<TaskListCreateInput>(input);
        if args.subject.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{}: {}", self.display_name(), args.subject)
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Compact,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Hidden,
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskListCreate",
    display: || Box::new(TaskListCreateDisplay)
});

// ── TaskListComplete ─────────────────────────────────────────────

struct TaskListCompleteDisplay;
impl ToolDisplay for TaskListCompleteDisplay {
    fn name(&self) -> &str {
        "TaskListComplete"
    }
    fn format_header(&self, _input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        self.display_name().to_string()
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Hidden,
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskListComplete",
    display: || Box::new(TaskListCompleteDisplay)
});

// ── TaskGet ──────────────────────────────────────────────────────

struct TaskGetDisplay;
impl ToolDisplay for TaskGetDisplay {
    fn name(&self) -> &str {
        "TaskGet"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<TaskGetInput>(input);
        if args.task_id.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {}", self.display_name(), args.task_id)
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn format_result_lines(&self, content: Option<&serde_json::Value>) -> Option<Vec<String>> {
        let task = content?.get("task")?;
        let mut lines = vec![format_task_view_line(task)];
        let description = json_str(task, "description").trim();
        if !description.is_empty() && description != "-" {
            lines.push(truncate_ellipsis(description, 100));
        }
        Some(lines)
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Visible {
                max_lines: Some(8),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskGet",
    display: || Box::new(TaskGetDisplay)
});

// ── TaskStop ─────────────────────────────────────────────────────

struct TaskStopDisplay;
impl ToolDisplay for TaskStopDisplay {
    fn name(&self) -> &str {
        "TaskStop"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let args = parse_input::<TaskStopInput>(input);
        if args.task_id.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {}", self.display_name(), args.task_id)
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Hidden,
            result: ResultPolicy::Hidden,
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskStop",
    display: || Box::new(TaskStopDisplay)
});

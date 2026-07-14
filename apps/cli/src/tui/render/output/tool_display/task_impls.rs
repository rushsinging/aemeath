use super::common::{bool_arg, str_arg, truncate_ellipsis, typed_data};
use super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};
use crate::tui::render::theme;
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use sdk::tool_result::TaskUpdateResult;
use std::path::Path;

// ── TaskCreate ───────────────────────────────────────────────────

struct TaskCreateDisplay;
impl ToolDisplay for TaskCreateDisplay {
    fn name(&self) -> &str {
        "TaskCreate"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let subject = str_arg(input, "subject", "");
        if subject.is_empty() {
            return self.display_name().to_string();
        }
        let desc = str_arg(input, "description", "");
        if desc.is_empty() {
            format!("{} {subject}", self.display_name())
        } else {
            format!(
                "{} {subject}: {}",
                self.display_name(),
                truncate_ellipsis(desc, 60)
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
        let id = str_arg(input, "taskId", "");
        if id.is_empty() {
            return self.display_name().to_string();
        }
        let summary = self.header_summary(input, None);
        match summary.is_empty() {
            false => format!("{} {id} — {}", self.display_name(), summary),
            true => format!("{} {id}", self.display_name()),
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
    /// LLM 调用 TaskUpdate 时通常只传 taskId + status，subject 在 TaskCreate
    /// 时设定，只有 store 回填的 result 才有 → 故必须覆写此方法。
    fn format_header_line_with_result(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
        workspace_root: Option<&Path>,
    ) -> Line<'static> {
        let id = str_arg(input, "taskId", "");
        if id.is_empty() {
            return self.format_header_line(input, workspace_root);
        }
        let summary = self.header_summary(input, result_payload);
        let name = self.display_name().to_string();
        let mut spans = vec![
            Span::styled(name, Style::default().fg(theme::ACCENT_BRIGHT)),
            Span::raw(format!(" {id}")),
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
    fn header_summary(
        &self,
        input: &serde_json::Value,
        result_payload: Option<&ToolResultPayload>,
    ) -> String {
        let key = str_arg(input, "key", "");
        let value = input.get("value");

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
        match key {
            "status" => {
                if let Some(s) = value.and_then(|v| v.as_str()) {
                    parts.push(format!("→ {s}"));
                }
            }
            "priority" => {
                if let Some(s) = value.and_then(|v| v.as_str()) {
                    parts.push(format!("p={s}"));
                }
            }
            "blocked_by_id" => {
                if let Some(s) = value.and_then(|v| v.as_str()) {
                    parts.push(format!("blocked by #{s}"));
                }
            }
            "subject" | "description" | "owner" => {
                // 这些字段变更不额外展示在 header，subject 从 result 回填即可
            }
            _ => {}
        }
        parts.join(", ")
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskUpdate",
    display: || Box::new(TaskUpdateDisplay)
});

// ── TaskList ─────────────────────────────────────────────────────

struct TaskListDisplay;
impl ToolDisplay for TaskListDisplay {
    fn name(&self) -> &str {
        "TaskList"
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
            result: ResultPolicy::Visible {
                max_lines: Some(5),
                render_kind: ResultRender::Plain,
                tail_mode: false,
            },
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskList",
    display: || Box::new(TaskListDisplay)
});

// ── TaskListCreate ───────────────────────────────────────────────

struct TaskListCreateDisplay;
impl ToolDisplay for TaskListCreateDisplay {
    fn name(&self) -> &str {
        "TaskListCreate"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let subject = str_arg(input, "subject", "");
        if subject.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{}: {subject}", self.display_name())
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

// ── Skill ────────────────────────────────────────────────────────

struct SkillDisplay;
impl ToolDisplay for SkillDisplay {
    fn name(&self) -> &str {
        "Skill"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let skill = str_arg(input, "skill", "");
        if skill.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {skill}", self.display_name())
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
    name: "Skill",
    display: || Box::new(SkillDisplay)
});

// ── LSP ──────────────────────────────────────────────────────────

struct LspDisplay;
impl ToolDisplay for LspDisplay {
    fn name(&self) -> &str {
        "LSP"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let op = str_arg(input, "operation", "");
        let path = str_arg(input, "filePath", "");
        let name = self.display_name();
        match (op.is_empty(), path.is_empty()) {
            (true, true) => name.to_string(),
            (true, false) => format!("{name} {path}"),
            (false, true) => format!("{name}::{op}"),
            (false, false) => format!("{name}::{op} {path}"),
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
    name: "LSP",
    display: || Box::new(LspDisplay)
});

// ── TaskGet ──────────────────────────────────────────────────────

struct TaskGetDisplay;
impl ToolDisplay for TaskGetDisplay {
    fn name(&self) -> &str {
        "TaskGet"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let id = str_arg(input, "taskId", "");
        if id.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {id}", self.display_name())
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
        let id = str_arg(input, "taskId", "");
        if id.is_empty() {
            self.display_name().to_string()
        } else {
            format!("{} {id}", self.display_name())
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

// ── EnterPlanMode ────────────────────────────────────────────────

struct EnterPlanModeDisplay;
impl ToolDisplay for EnterPlanModeDisplay {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        let reason = str_arg(input, "reason", "");
        if reason.is_empty() {
            self.display_name().to_string()
        } else {
            format!("Plan: {reason}")
        }
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec!["Tool calls will be simulated, not executed.".to_string()]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::CustomIcon("📋"),
            details: DetailsPolicy::Expanded,
            result: ResultPolicy::Hidden,
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "EnterPlanMode",
    display: || Box::new(EnterPlanModeDisplay)
});

// ── ExitPlanMode ─────────────────────────────────────────────────

struct ExitPlanModeDisplay;
impl ToolDisplay for ExitPlanModeDisplay {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }
    fn format_header(&self, input: &serde_json::Value, _workspace_root: Option<&Path>) -> String {
        if bool_arg(input, "execute", false) {
            "Execute Plan".to_string()
        } else {
            self.display_name().to_string()
        }
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        if bool_arg(input, "execute", false) {
            vec!["Planned actions will now be executed.".to_string()]
        } else {
            vec!["Returning to normal execution.".to_string()]
        }
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::CustomIcon("▶"),
            details: DetailsPolicy::Expanded,
            result: ResultPolicy::Hidden,
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "ExitPlanMode",
    display: || Box::new(ExitPlanModeDisplay)
});

use super::common::{bool_arg, str_arg, truncate_ellipsis};
use super::{
    DetailsPolicy, HeaderPolicy, ResultPolicy, ResultRender, ToolDisplay, ToolDisplayEntry,
    ToolRenderPolicy,
};

// ── TaskCreate ───────────────────────────────────────────────────

struct TaskCreateDisplay;
impl ToolDisplay for TaskCreateDisplay {
    fn name(&self) -> &str {
        "TaskCreate"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
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
    fn format_header(&self, input: &serde_json::Value) -> String {
        let id = str_arg(input, "taskId", "");
        if id.is_empty() {
            return self.display_name().to_string();
        }
        let status = str_arg(input, "status", "");
        let blocked_by = input
            .get("addBlockedBy")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let subject = str_arg(input, "subject", "");
        let priority = str_arg(input, "priority", "");
        let progress = input.get("progress").and_then(|v| v.as_u64());

        // 构建摘要片段（按重要性排序）
        let mut parts = Vec::new();
        if !status.is_empty() {
            parts.push(format!("→ {status}"));
        }
        if !blocked_by.is_empty() {
            parts.push(format!("blocked by [{blocked_by}]"));
        }
        if !subject.is_empty() {
            parts.push(truncate_ellipsis(subject, 40));
        }
        if !priority.is_empty() {
            parts.push(format!("p={priority}"));
        }
        if let Some(pct) = progress {
            parts.push(format!("{pct}%"));
        }

        if parts.is_empty() {
            format!("{} {id}", self.display_name())
        } else {
            format!("{} {id} — {}", self.display_name(), parts.join(", "))
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
    name: "TaskUpdate",
    display: || Box::new(TaskUpdateDisplay)
});

// ── TaskList ─────────────────────────────────────────────────────

struct TaskListDisplay;
impl ToolDisplay for TaskListDisplay {
    fn name(&self) -> &str {
        "TaskList"
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
    fn format_header(&self, input: &serde_json::Value) -> String {
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
    fn format_header(&self, input: &serde_json::Value) -> String {
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
    fn format_header(&self, input: &serde_json::Value) -> String {
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
    fn format_header(&self, input: &serde_json::Value) -> String {
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
    fn format_header(&self, input: &serde_json::Value) -> String {
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
    fn format_header(&self, input: &serde_json::Value) -> String {
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
    fn format_header(&self, input: &serde_json::Value) -> String {
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

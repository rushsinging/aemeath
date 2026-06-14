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
    fn format_header(&self, input: &serde_json::Value, _summary: Option<&str>) -> String {
        let subject = str_arg(input, "subject", "?");
        format!("TaskCreate {subject}")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let desc = str_arg(input, "description", "");
        if desc.is_empty() {
            return vec![];
        }
        vec![truncate_ellipsis(desc, 80)]
    }
    fn render_policy(&self) -> ToolRenderPolicy {
        ToolRenderPolicy {
            header: HeaderPolicy::Standard,
            details: DetailsPolicy::Expanded,
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
    fn format_header(&self, input: &serde_json::Value, _summary: Option<&str>) -> String {
        let id = str_arg(input, "taskId", "?");
        let status = str_arg(input, "status", "");
        if status.is_empty() {
            format!("TaskUpdate {id}")
        } else {
            format!("TaskUpdate {id} → {status}")
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
    fn format_header(&self, _input: &serde_json::Value, _summary: Option<&str>) -> String {
        "TaskList".to_string()
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
    fn format_header(&self, input: &serde_json::Value, _summary: Option<&str>) -> String {
        let subject = str_arg(input, "subject", "?");
        format!("TaskListCreate: {subject}")
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
    fn format_header(&self, _input: &serde_json::Value, _summary: Option<&str>) -> String {
        "TaskListComplete".to_string()
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
    fn format_header(&self, input: &serde_json::Value, _summary: Option<&str>) -> String {
        let skill = str_arg(input, "skill", "?");
        format!("Skill {skill}")
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
    fn format_header(&self, input: &serde_json::Value, _summary: Option<&str>) -> String {
        let op = str_arg(input, "operation", "?");
        let path = str_arg(input, "filePath", "?");
        format!("LSP::{op} {path}")
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
    fn format_header(&self, input: &serde_json::Value, _summary: Option<&str>) -> String {
        let id = str_arg(input, "taskId", "?");
        format!("TaskGet {id}")
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
    fn format_header(&self, input: &serde_json::Value, _summary: Option<&str>) -> String {
        let id = str_arg(input, "taskId", "?");
        format!("TaskStop {id}")
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

// ── TaskOutput ───────────────────────────────────────────────────

struct TaskOutputDisplay;
impl ToolDisplay for TaskOutputDisplay {
    fn name(&self) -> &str {
        "TaskOutput"
    }
    fn format_header(&self, _input: &serde_json::Value, _summary: Option<&str>) -> String {
        "TaskOutput".to_string()
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
    name: "TaskOutput",
    display: || Box::new(TaskOutputDisplay)
});

// ── EnterPlanMode ────────────────────────────────────────────────

struct EnterPlanModeDisplay;
impl ToolDisplay for EnterPlanModeDisplay {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }
    fn format_header(&self, input: &serde_json::Value, _summary: Option<&str>) -> String {
        let reason = str_arg(input, "reason", "");
        if reason.is_empty() {
            "📋 Enter Plan Mode".to_string()
        } else {
            format!("📋 Plan: {reason}")
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
    fn format_header(&self, input: &serde_json::Value, _summary: Option<&str>) -> String {
        if bool_arg(input, "execute", false) {
            "▶ Execute Plan".to_string()
        } else {
            "▶ Exit Plan Mode".to_string()
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

use super::common::{bool_arg, str_arg, truncate_ellipsis};
use super::{ToolDisplay, ToolDisplayEntry, TOOL_RESULT_MAX_LINES};

struct TaskCreateDisplay;
impl ToolDisplay for TaskCreateDisplay {
    fn name(&self) -> &str {
        "TaskCreate"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let subject = str_arg(input, "subject", "?");
        format!("● TaskCreate({subject})")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let desc = str_arg(input, "description", "");
        if desc.is_empty() {
            return vec![];
        }
        vec![truncate_ellipsis(desc, 80)]
    }
    fn result_max_lines(&self) -> usize {
        TOOL_RESULT_MAX_LINES
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
        let id = str_arg(input, "taskId", "?");
        format!("● TaskUpdate({id})")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let status = str_arg(input, "status", "");
        if status.is_empty() {
            return vec![];
        }
        vec![format!("-> {status}")]
    }
    fn result_max_lines(&self) -> usize {
        TOOL_RESULT_MAX_LINES
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
        TOOL_RESULT_MAX_LINES
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskList",
    display: || Box::new(TaskListDisplay)
});

struct TaskListCreateDisplay;
impl ToolDisplay for TaskListCreateDisplay {
    fn name(&self) -> &str {
        "TaskListCreate"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let subject = str_arg(input, "subject", "?");
        format!("● TaskListCreate: {subject}")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let summary = str_arg(input, "summary", "");
        if summary.is_empty() {
            vec![]
        } else {
            vec![truncate_ellipsis(summary, 80)]
        }
    }
    fn result_max_lines(&self) -> usize {
        0
    }
    fn format_result_summary(&self, _result: &str, _is_error: bool) -> Vec<String> {
        vec![]
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskListCreate",
    display: || Box::new(TaskListCreateDisplay)
});

struct TaskListCompleteDisplay;
impl ToolDisplay for TaskListCompleteDisplay {
    fn name(&self) -> &str {
        "TaskListComplete"
    }
    fn format_header(&self, _input: &serde_json::Value) -> String {
        "● TaskListComplete".to_string()
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn result_max_lines(&self) -> usize {
        0
    }
    fn format_result_summary(&self, _result: &str, _is_error: bool) -> Vec<String> {
        vec![]
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "TaskListComplete",
    display: || Box::new(TaskListCompleteDisplay)
});

struct SkillDisplay;
impl ToolDisplay for SkillDisplay {
    fn name(&self) -> &str {
        "Skill"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let skill = str_arg(input, "skill", "?");
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
        let op = str_arg(input, "operation", "?");
        let path = str_arg(input, "filePath", "?");
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
        let id = str_arg(input, "taskId", "?");
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
        let id = str_arg(input, "taskId", "?");
        format!("● TaskStop({id})")
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
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

use std::collections::HashMap;
use std::sync::LazyLock;

mod common;
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

    /// Max lines of result output to show (default 5).
    #[allow(dead_code)]
    fn result_max_lines(&self) -> usize {
        TOOL_RESULT_MAX_LINES
    }

    /// Format the result summary line(s). Default: "✓ {name} completed".
    #[allow(dead_code)]
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

pub(crate) fn lookup_display(name: &str) -> Option<&'static dyn ToolDisplay> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_display_finds_task_list_create() {
        let display = lookup_display("TaskListCreate");
        assert!(display.is_some(), "TaskListCreate 应在 display registry 中注册");
        assert_eq!(display.unwrap().name(), "TaskListCreate");
    }

    #[test]
    fn test_lookup_display_finds_task_create() {
        assert!(lookup_display("TaskCreate").is_some());
    }

    #[test]
    fn test_lookup_display_finds_task_update() {
        assert!(lookup_display("TaskUpdate").is_some());
    }

    #[test]
    fn test_format_tool_call_task_list_create() {
        let (header, details) =
            format_tool_call("TaskListCreate", r#"{"subject":"修复 bug 84","summary":"修复渲染"}"#);
        assert!(header.contains("修复 bug 84"), "header 应包含 subject: {header}");
        assert!(!details.is_empty(), "details 应包含 summary");
    }

    #[test]
    fn test_format_tool_call_task_create() {
        let (header, details) =
            format_tool_call("TaskCreate", r#"{"subject":"分析","description":"查看结构"}"#);
        assert!(header.contains("分析"), "header: {header}");
        assert!(!details.is_empty());
    }

    #[test]
    fn test_format_tool_call_unknown_tool_uses_fallback() {
        let (header, details) = format_tool_call("UnknownTool", r#"{"key":"value"}"#);
        assert_eq!(header, "● UnknownTool");
        assert!(!details.is_empty(), "fallback 应截断 JSON");
    }

    #[test]
    fn test_format_tool_call_invalid_json_uses_fallback() {
        let (header, _details) = format_tool_call("TaskListCreate", "not json");
        // 不应 panic，应 fallback
        assert!(header.contains("TaskListCreate"));
    }
}

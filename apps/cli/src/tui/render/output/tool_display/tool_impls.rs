use crate::tui::output_area::{display, LineStyle, INDENT};

use super::common::{file_path, str_arg, truncate_ellipsis, u64_arg};
use super::{ToolDisplay, ToolDisplayEntry, TOOL_RESULT_MAX_LINES};

struct BashDisplay;
impl ToolDisplay for BashDisplay {
    fn name(&self) -> &str {
        "Bash"
    }
    fn format_header(&self, _input: &serde_json::Value) -> String {
        "● Bash".to_string()
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let cmd = str_arg(input, "command", "?");
        let timeout = u64_arg(input, "timeout");
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
        let path = file_path(input);
        format!("● Read({path})")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let path = file_path(input);
        let offset = u64_arg(input, "offset");
        let limit = u64_arg(input, "limit");
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
        let path = file_path(input);
        format!("● Write({path})")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let content = str_arg(input, "content", "");
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
        let path = file_path(input);
        format!("● Edit({path})")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let old = str_arg(input, "old_string", "");
        let new = str_arg(input, "new_string", "");
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
        let pattern = str_arg(input, "pattern", "?");
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
        let pattern = str_arg(input, "pattern", "?");
        format!("● Grep /{pattern}/")
    }
    fn format_details(&self, input: &serde_json::Value) -> Vec<String> {
        let path = str_arg(input, "path", ".");
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
        let desc = str_arg(input, "description", "sub-task");
        let role = input.get("role").and_then(|role| role.as_str());
        let model = input.get("model").and_then(|model| model.as_str());
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
        let prompt = str_arg(input, "prompt", "");
        if prompt.is_empty() {
            return vec![];
        }
        vec![truncate_ellipsis(
            prompt,
            200usize.saturating_sub(INDENT.len()),
        )]
    }
    fn result_max_lines(&self) -> usize {
        TOOL_RESULT_MAX_LINES
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
        let url = str_arg(input, "url", "?");
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

struct AskUserQuestionDisplay;
impl ToolDisplay for AskUserQuestionDisplay {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }
    fn format_header(&self, input: &serde_json::Value) -> String {
        let question = str_arg(input, "question", "?");
        let preview = truncate_ellipsis(question, 60usize.saturating_sub(INDENT.len()));
        format!("● AskUserQuestion({preview})")
    }
    fn format_details(&self, _input: &serde_json::Value) -> Vec<String> {
        vec![]
    }
    fn result_max_lines(&self) -> usize {
        // answer is already shown via push_user_message; suppress redundant display
        0
    }
    fn format_result_summary(&self, _result: &str, is_error: bool) -> Vec<String> {
        if is_error {
            vec!["✗ 回答失败".to_string()]
        } else {
            vec!["✓ 已回答".to_string()]
        }
    }
}
inventory::submit!(ToolDisplayEntry {
    name: "AskUserQuestion",
    display: || Box::new(AskUserQuestionDisplay)
});

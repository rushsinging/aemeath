use sdk::{AgentProgressEventView, AgentProgressKindView};
use serde_json::Value;

/// 把 AgentProgressEventView 格式化为人类可读消息，供 TUI activities 渲染。
pub(super) fn format_agent_progress(event: &AgentProgressEventView) -> String {
    match &event.kind {
        AgentProgressKindView::Started { .. } => String::new(),
        AgentProgressKindView::Message { text } => text.clone(),
        AgentProgressKindView::ToolCalls { calls } => {
            if calls.is_empty() {
                return String::new();
            }
            let lines: Vec<String> = calls
                .iter()
                .map(|tc| {
                    let input_preview = match &tc.input {
                        Value::String(s) => s.chars().take(80).collect::<String>(),
                        v => {
                            let s = v.to_string();
                            s.chars().take(80).collect::<String>()
                        }
                    };
                    if input_preview.is_empty() {
                        format!("→ {}", tc.name)
                    } else {
                        format!("→ {}  {}", tc.name, input_preview)
                    }
                })
                .collect();
            lines.join("\n")
        }
    }
}

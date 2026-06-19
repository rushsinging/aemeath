#[derive(Debug, Clone)]
pub struct ImageData {
    pub base64: String,
    pub media_type: String,
}

/// Typed result structs for tools, one per tool. These are the canonical,
/// "horizontal shared" representation of what a tool produced; the same types
/// are re-exported by `packages/sdk::tool_result` for `cli` and the future
/// `server` consumer, while tools themselves reference them via
/// `share::tool::types::XxxResult`.
///
/// See `docs/superpowers/plans/2026-06-18-tool-display-structured-data.md`
/// (plan 方案 D) for the design rationale.
pub mod types;

// ---------------------------------------------------------------------------
// Path-policy types (shared so both `tools` and `policy` can reference them
// without depending on each other).
// ---------------------------------------------------------------------------

/// Kind of path access a tool declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// Single file read/write.
    File,
    /// Directory to search.
    SearchDir,
}

/// A declared path field inside a tool's input JSON.
#[derive(Debug, Clone, Copy)]
pub struct PathAccess {
    /// JSON field name in the tool input (e.g. `"file_path"`, `"path"`).
    pub field: &'static str,
    /// How the path should be validated.
    pub kind: PathKind,
}

/// The outcome of evaluating a single tool call against policy.
#[derive(Debug)]
pub enum PolicyDecision {
    /// The call is allowed; `input` has been normalised (paths resolved).
    Allow(serde_json::Value),
    /// The call is denied; `reason` explains why.
    Deny { reason: String },
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub output: String,
    pub content: serde_json::Value,
    pub is_error: bool,
    /// Optional images to include in the tool result (for vision-capable models)
    pub images: Vec<ImageData>,
}

impl ToolResult {
    pub fn success(output: impl Into<String>) -> Self {
        Self::text(output, false)
    }

    pub fn error(output: impl Into<String>) -> Self {
        Self::text(output, true)
    }

    pub fn success_json(content: serde_json::Value) -> Self {
        Self::json(content, false)
    }

    pub fn error_json(content: serde_json::Value) -> Self {
        Self::json(content, true)
    }

    pub fn text(output: impl Into<String>, is_error: bool) -> Self {
        let output = output.into();
        Self {
            content: serde_json::json!({ "text": output }),
            output,
            is_error,
            images: Vec::new(),
        }
    }

    pub fn json(content: serde_json::Value, is_error: bool) -> Self {
        let output = display_text_from_content(&content);
        Self {
            output,
            content,
            is_error,
            images: Vec::new(),
        }
    }

    pub fn with_image(mut self, base64: String, media_type: String) -> Self {
        self.images.push(ImageData { base64, media_type });
        self
    }

    /// 获取显示文本（从 content 中提取）
    /// 优先级：display > message > text > 序列化 JSON
    pub fn display_text(&self) -> String {
        display_text_from_content(&self.content)
    }
}

fn display_text_from_content(content: &serde_json::Value) -> String {
    if let Some(display) = content.get("display").and_then(|value| value.as_str()) {
        return display.to_string();
    }
    if let Some(message) = content.get("message").and_then(|value| value.as_str()) {
        return message.to_string();
    }
    if let Some(text) = content.get("text").and_then(|value| value.as_str()) {
        return text.to_string();
    }
    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_result_success_wraps_text_payload() {
        let result = ToolResult::success("ok");

        assert_eq!(result.output, "ok");
        assert!(!result.is_error);
        assert_eq!(result.content, serde_json::json!({ "text": "ok" }));
    }

    #[test]
    fn test_tool_result_json_prefers_display_text() {
        let result = ToolResult::success_json(serde_json::json!({
            "display": "shown in tui",
            "message": "message for llm",
            "data": { "value": 1 }
        }));

        assert_eq!(result.output, "shown in tui");
        assert_eq!(result.content["data"]["value"], 1);
    }

    #[test]
    fn test_tool_result_json_falls_back_to_message_text_or_serialized_json() {
        let message = ToolResult::success_json(serde_json::json!({ "message": "msg" }));
        let text = ToolResult::success_json(serde_json::json!({ "text": "txt" }));
        let other = ToolResult::error_json(serde_json::json!({ "items": [1, 2] }));

        assert_eq!(message.output, "msg");
        assert_eq!(text.output, "txt");
        assert_eq!(other.output, r#"{"items":[1,2]}"#);
        assert!(other.is_error);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentProgressEvent {
    /// Monotonic sequence for internal ordering/replacement. UI does not display it by default.
    pub sequence: usize,
    pub kind: AgentProgressKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentProgressKind {
    ToolCalls { calls: Vec<AgentToolCallProgress> },
    Message { text: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentToolCallProgress {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct SessionReminder {
    pub id: String,
    pub content: String,
    pub done: bool,
    pub created_at: u64,
}

#[derive(Debug, Default, Clone)]
pub struct SessionReminders {
    reminders: Vec<SessionReminder>,
}

impl SessionReminders {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(
        &mut self,
        id: impl Into<String>,
        content: impl Into<String>,
        created_at: u64,
    ) -> Result<String, String> {
        let id = id.into();
        let content = content.into();
        if id.trim().is_empty() {
            return Err("reminder id 不能为空".to_string());
        }
        if content.trim().is_empty() {
            return Err("reminder 内容不能为空".to_string());
        }

        self.reminders.push(SessionReminder {
            id: id.clone(),
            content,
            done: false,
            created_at,
        });
        Ok(id)
    }

    pub fn complete(&mut self, id: &str) -> Result<(), String> {
        let reminder = self
            .reminders
            .iter_mut()
            .find(|reminder| reminder.id == id)
            .ok_or_else(|| format!("memory not found: {id}"))?;
        reminder.done = true;
        Ok(())
    }

    pub fn list(&self) -> &[SessionReminder] {
        &self.reminders
    }

    pub fn clear(&mut self) {
        self.reminders.clear();
    }

    pub fn recap_line(&self) -> Option<String> {
        let active = self
            .reminders
            .iter()
            .filter(|reminder| !reminder.done)
            .map(|reminder| reminder.content.as_str())
            .collect::<Vec<_>>();

        if active.is_empty() {
            None
        } else {
            Some(format!("* recap: {}", active.join(" | ")))
        }
    }
}

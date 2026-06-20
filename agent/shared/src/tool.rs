#[derive(Debug, Clone)]
pub struct ImageData {
    pub base64: String,
    pub media_type: String,
}

/// 工具执行结果：统一、具体、非泛型。runtime 管线全程传递它，取代历史的 6 元组
/// （`ToolResultTuple` / `UiToolResult`）。
///
/// - `text`：给 LLM 的文本（text-first）+ TUI 预览；LLM 唯一读到的内容。
/// - `data`：结构化结果（TUI / server 边界按 `tool_name` 的 Output schema 反序列化），
///   无结构化时为 `Value::Null`。
/// - `images`：多模态，有图时随 `text` 组成 wire 多块数组。
///
/// 设计见 `docs/superpowers/specs/2026-06-19-tool-pipeline-typed-refactor-design.md`。
#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub text: String,
    pub data: serde_json::Value,
    pub is_error: bool,
    pub images: Vec<ImageData>,
}

impl ToolOutcome {
    /// 成功/正常结果（含结构化 `data`）。
    pub fn new(text: impl Into<String>, data: serde_json::Value, images: Vec<ImageData>) -> Self {
        Self {
            text: text.into(),
            data,
            is_error: false,
            images,
        }
    }

    /// 错误结果。
    ///
    /// Phase A 行为保持：`data` 暂用 `{"text": msg}`（与旧 wire 形态一致）。
    /// Phase B 切 text-first 后，LLM 走 `text`、此处 `data` 可收敛为 `Null`。
    pub fn error(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            data: serde_json::json!({ "text": text }),
            text,
            is_error: true,
            images: Vec::new(),
        }
    }

    /// 从工具返回的 [`ToolResult`] 映射（保持现有 `data = content` 行为）。
    pub fn from_tool_result(r: ToolResult) -> Self {
        Self {
            text: r.output,
            data: r.content,
            is_error: r.is_error,
            images: r.images,
        }
    }
}

#[cfg(test)]
mod tool_outcome_tests {
    use super::ToolOutcome;

    #[test]
    fn test_tool_outcome_new_normal() {
        let o = ToolOutcome::new("ok", serde_json::json!({"n": 1}), vec![]);
        assert_eq!(o.text, "ok");
        assert_eq!(o.data, serde_json::json!({"n": 1}));
        assert!(!o.is_error);
        assert!(o.images.is_empty());
    }

    #[test]
    fn test_tool_outcome_error_keeps_text_in_data() {
        // 边界/错误路径：error 构造保持 data = {"text": msg}，与旧 wire 一致。
        let o = ToolOutcome::error("boom");
        assert_eq!(o.text, "boom");
        assert!(o.is_error);
        assert_eq!(o.data, serde_json::json!({"text": "boom"}));
    }

    #[test]
    fn test_tool_outcome_from_tool_result_maps_fields() {
        let r = super::ToolResult {
            output: "out".to_string(),
            content: serde_json::json!({"k": "v"}),
            is_error: false,
            images: vec![],
            data: None,
        };
        let o = ToolOutcome::from_tool_result(r);
        assert_eq!(o.text, "out");
        assert_eq!(o.data, serde_json::json!({"k": "v"}));
        assert!(!o.is_error);
    }
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

/// Generic tool result, parameterised over the typed payload `R`.
///
/// The default `R = serde_json::Value` preserves full backward
/// compatibility: existing call sites that build
/// `ToolResult::success(...)` / `ToolResult::error(...)` keep
/// compiling without change.
///
/// Going forward, a tool `T` declares
/// `impl Tool for T { type Result = ToolResult<MyResult>; }` so that
/// downstream consumers (TUI, server, persistence) can read the typed
/// payload directly from `ToolResult::data`.
///
/// `data` is wrapped in `Option<R>` so that:
///
/// - the constructors (`success`, `error`, `text`) populate the
///   legacy `output` / `content` fields without forcing every `R` to
///   implement `Default`, and
/// - new tool impls that opt into a typed `R` can still leave `data`
///   as `None` while the migration to typed payloads is in flight.
///
/// See `docs/superpowers/plans/2026-06-18-tool-display-structured-data.md`
/// (plan 方案 D) for the design rationale.
#[derive(Debug, Clone)]
pub struct ToolResult<R = serde_json::Value> {
    pub output: String,
    pub content: serde_json::Value,
    pub is_error: bool,
    /// Optional images to include in the tool result (for vision-capable models)
    pub images: Vec<ImageData>,
    /// Typed payload (see struct docs).
    pub data: Option<R>,
}

impl<R> ToolResult<R> {
    pub fn success(output: impl Into<String>) -> Self {
        Self::text(output, false)
    }

    pub fn error(output: impl Into<String>) -> Self {
        Self::text(output, true)
    }

    pub fn text(output: impl Into<String>, is_error: bool) -> Self {
        let output = output.into();
        Self {
            content: serde_json::json!({ "text": output }),
            output,
            is_error,
            images: Vec::new(),
            data: None,
        }
    }

    pub fn with_image(mut self, base64: String, media_type: String) -> Self {
        self.images.push(ImageData { base64, media_type });
        self
    }

    /// Attach a typed payload to this result.
    pub fn with_data(mut self, data: R) -> Self {
        self.data = Some(data);
        self
    }

    /// 获取显示文本（从 content 中提取）
    /// 优先级：display > message > text > 序列化 JSON
    pub fn display_text(&self) -> String {
        display_text_from_content(&self.content)
    }
}

// Manual `Default` impl covers any `R`. The legacy `data: Value` ergonomics
// are preserved by initialising `data` to `None` (see the constructors
// above) and letting callers opt-in with `with_data`.
impl<R> Default for ToolResult<R> {
    fn default() -> Self {
        Self {
            output: String::new(),
            content: serde_json::Value::Null,
            is_error: false,
            images: Vec::new(),
            data: None,
        }
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
        let result: ToolResult = ToolResult::success("ok");

        assert_eq!(result.output, "ok");
        assert!(!result.is_error);
        assert_eq!(result.content, serde_json::json!({ "text": "ok" }));
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

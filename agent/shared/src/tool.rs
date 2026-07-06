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

    /// 从工具返回的 [`ToolResult`] 映射（1:1，字段命名已对齐）。
    pub fn from_tool_result(r: ToolResult) -> Self {
        Self {
            text: r.text,
            data: r.data,
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
            text: "out".to_string(),
            data: serde_json::json!({"k": "v"}),
            is_error: false,
            images: vec![],
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

/// Tool execution result（执行态，非泛型）。
///
/// 字段命名与 [`ToolOutcome`] 完全对齐：`text→LLM / data→TUI`。
/// `Tool::call` 返回本类型；`ToolOutcome::from_tool_result` 直接 1:1 映射。
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// 给 LLM 的文本。
    pub text: String,
    /// 结构化数据（给 TUI 反序列化渲染）。
    pub data: serde_json::Value,
    pub is_error: bool,
    /// Optional images to include in the tool result (for vision-capable models)
    pub images: Vec<ImageData>,
}

impl ToolResult {
    pub fn success(text: impl Into<String>) -> Self {
        Self::text(text, false)
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::text(text, true)
    }

    pub fn text(text: impl Into<String>, is_error: bool) -> Self {
        Self {
            text: text.into(),
            data: serde_json::Value::Null,
            is_error,
            images: Vec::new(),
        }
    }

    pub fn with_image(mut self, base64: String, media_type: String) -> Self {
        self.images.push(ImageData { base64, media_type });
        self
    }
}

impl Default for ToolResult {
    fn default() -> Self {
        Self {
            text: String::new(),
            data: serde_json::Value::Null,
            is_error: false,
            images: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_result_success_wraps_text_payload() {
        let result: ToolResult = ToolResult::success("ok");

        assert_eq!(result.text, "ok");
        assert!(!result.is_error);
        assert_eq!(result.data, serde_json::Value::Null);
    }

    // issue #646：AgentProgressKind::Started 构造/字段/PartialEq 验证
    #[test]
    fn test_agent_progress_started_with_role() {
        let ev = AgentProgressEvent {
            sequence: 0,
            kind: AgentProgressKind::Started {
                role: Some("coder".into()),
                model: "Zhipu/glm-5.2".into(),
            },
        };
        match &ev.kind {
            AgentProgressKind::Started { role, model } => {
                assert_eq!(role.as_deref(), Some("coder"));
                assert_eq!(model, "Zhipu/glm-5.2");
            }
            _ => panic!("expected Started"),
        }
    }

    #[test]
    fn test_agent_progress_started_without_role() {
        let ev = AgentProgressEvent {
            sequence: 0,
            kind: AgentProgressKind::Started {
                role: None,
                model: "default-model".into(),
            },
        };
        match &ev.kind {
            AgentProgressKind::Started { role, model } => {
                assert!(role.is_none());
                assert_eq!(model, "default-model");
            }
            _ => panic!("expected Started"),
        }
    }

    #[test]
    fn test_agent_progress_kind_partial_eq() {
        let a = AgentProgressKind::Started {
            role: None,
            model: "x".into(),
        };
        let b = AgentProgressKind::Started {
            role: None,
            model: "x".into(),
        };
        assert_eq!(a, b);

        let c = AgentProgressKind::Started {
            role: Some("y".into()),
            model: "x".into(),
        };
        assert_ne!(a, c);

        // 不同变体不相等
        let d = AgentProgressKind::Message { text: "x".into() };
        assert_ne!(a, d);
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
    /// Sub-agent 启动时发出（issue #499）。携带实际 resolve 后的 role/model，
    /// 让 TUI 在 Agent 工具 header 显示 `Agent - [role] - Provider/model`。
    /// 早于 ToolCalls/Message 发出，是 sub-agent 的第一个 progress 事件。
    Started {
        role: Option<String>,
        model: String,
    },
    ToolCalls {
        calls: Vec<AgentToolCallProgress>,
    },
    Message {
        text: String,
    },
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

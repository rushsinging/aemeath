use super::ids::{ToolCallId, ToolStreamKey};
use super::streaming_preview::ToolStreamingPreviewBuffer;
use super::tool_result_payload::ToolResultPayload;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolCall {
    pub id: Option<ToolCallId>,
    pub stream_key: ToolStreamKey,
    pub name: String,
    pub args_preview: String,
    pub status: ToolCallStatus,
    /// 工具执行结果（含 output/content/is_error/image_count 四字段）。
    /// None = 尚未收到结果；Some = 已完成（成功或失败）。
    pub result: Option<ToolResultPayload>,
    pub activities: Vec<String>,
    pub streaming_preview: Option<ToolStreamingPreviewBuffer>,
    /// Agent 工具特化元数据（issue #499）。仅 `tool_name == "Agent"` 时由
    /// `AgentProgressKind::Started` 事件填充，用于 header 渲染
    /// `Agent - [role] - Provider/model`。prompt 不在此处重复存储，
    /// 渲染时从 `args_preview` 取（已在 ToolCallUpdate status=Ready 时填充）。
    pub agent_meta: Option<AgentMeta>,
}

/// Agent 工具的元数据（issue #499）。
/// 由 runtime 的 `AgentProgressKind::Started` 事件携带，
/// 携带 sub-agent 实际 resolve 后的 role/model（而非 args 原始值）。
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AgentMeta {
    /// sub-agent 的角色名（如 `reviewer`）。None 表示未指定 role。
    pub role: Option<String>,
    /// sub-agent 实际使用的 model（如 `Zhipu/glm-5.2`），runtime resolve 后的值。
    pub model: String,
}

impl ToolCall {
    pub fn pending(id: ToolCallId, stream_key: ToolStreamKey) -> Self {
        Self {
            name: stream_key.name.clone(),
            id: Some(id),
            stream_key,
            args_preview: String::new(),
            status: ToolCallStatus::PendingArgs,
            result: None,
            activities: Vec::new(),
            streaming_preview: None,
            agent_meta: None,
        }
    }
    pub fn update_args(&mut self, partial_args: impl Into<String>) {
        let args = partial_args.into();
        if !args.is_empty() {
            self.args_preview = args;
        }
    }

    pub fn update(
        &mut self,
        arguments: Option<String>,
        status: ToolCallStatus,
    ) -> Vec<ToolCallChange> {
        if let Some(arguments) = arguments {
            if !arguments.is_empty() {
                self.args_preview = arguments;
            }
        }
        let previous = self.status;
        if self.status != ToolCallStatus::Success && self.status != ToolCallStatus::Error {
            self.status = status;
        }
        let mut changes = vec![ToolCallChange::Bound];
        if previous != status && status == ToolCallStatus::Running {
            changes.push(ToolCallChange::Running);
        }
        changes
    }
    pub fn bind(&mut self) -> Vec<ToolCallChange> {
        if self.status == ToolCallStatus::PendingArgs {
            self.status = ToolCallStatus::Running;
            vec![ToolCallChange::Bound, ToolCallChange::Running]
        } else {
            vec![ToolCallChange::Bound]
        }
    }
    pub fn complete(&mut self, result: ToolResultPayload) {
        let is_error = result.is_error;
        self.result = Some(result);
        self.status = if is_error {
            ToolCallStatus::Error
        } else {
            ToolCallStatus::Success
        };
    }

    pub fn orphan(&mut self) {
        self.status = ToolCallStatus::Orphaned;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolCallStatus {
    PendingArgs,
    Ready,
    Running,
    Success,
    Error,
    Cancelled,
    Orphaned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolCallChange {
    Bound,
    Running,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId, ToolStreamKey};

    fn stream_key() -> ToolStreamKey {
        ToolStreamKey::new(ChatId::new("chat-1"), ChatTurnId::new("turn-1"), "Read", 0)
    }

    use crate::tui::model::conversation::tool_result_payload::ToolResultPayload;

    fn pending_call() -> ToolCall {
        ToolCall::pending(ToolCallId::new("tool-1"), stream_key())
    }

    #[test]
    fn test_tool_call_binds_id_and_runs() {
        let mut call = pending_call();
        let changes = call.bind();
        assert!(call.id.as_ref().is_some(), "id should be set after bind");
        assert_eq!(call.status, ToolCallStatus::Running);
        assert_eq!(
            changes,
            vec![ToolCallChange::Bound, ToolCallChange::Running]
        );
    }

    #[test]
    fn test_tool_call_completes_success() {
        let mut call = pending_call();
        call.bind();
        let payload = ToolResultPayload::new(
            "ok".to_string(),
            serde_json::json!({ "text": "ok" }),
            false,
            0,
        );
        call.complete(payload.clone());
        assert_eq!(call.status, ToolCallStatus::Success);
        assert_eq!(call.result.as_ref().map(|p| p.output.as_str()), Some("ok"));
        assert_eq!(
            call.result.as_ref().map(|p| &p.content),
            Some(&serde_json::json!({ "text": "ok" }))
        );
        assert_eq!(call.result, Some(payload));
    }

    #[test]
    fn test_tool_call_completes_error() {
        let mut call = pending_call();
        call.bind();
        call.complete(ToolResultPayload::new(
            "failed".to_string(),
            serde_json::json!({ "text": "failed" }),
            true,
            0,
        ));
        assert_eq!(call.status, ToolCallStatus::Error);
        assert_eq!(
            call.result.as_ref().map(|p| p.output.as_str()),
            Some("failed")
        );
        assert!(call.result.as_ref().is_some_and(|p| p.is_error));
    }

    #[test]
    fn test_update_preserves_args_preview_when_arguments_none() {
        let mut call = pending_call();
        call.update(Some(r#"{"taskId":"42"}"#.into()), ToolCallStatus::Running);
        assert_eq!(call.args_preview, r#"{"taskId":"42"}"#);
        call.update(None, ToolCallStatus::Running);
        assert_eq!(
            call.args_preview, r#"{"taskId":"42"}"#,
            "None 不应覆盖已有 args_preview"
        );
    }

    #[test]
    fn test_update_preserves_args_preview_when_arguments_empty_string() {
        let mut call = pending_call();
        call.update(Some(r#"{"taskId":"42"}"#.into()), ToolCallStatus::Running);
        assert_eq!(call.args_preview, r#"{"taskId":"42"}"#);
        call.update(Some(String::new()), ToolCallStatus::Running);
        assert_eq!(
            call.args_preview, r#"{"taskId":"42"}"#,
            "空字符串不应覆盖已有 args_preview"
        );
    }

    // ── issue #839：update_args 同样需要空值防护 ──

    #[test]
    fn test_update_args_empty_does_not_overwrite() {
        let mut call = pending_call();
        call.update_args(r#"{"task_id":"42"}"#);
        call.update_args(""); // 空字符串应被忽略
        assert_eq!(call.args_preview, r#"{"task_id":"42"}"#);
    }
}

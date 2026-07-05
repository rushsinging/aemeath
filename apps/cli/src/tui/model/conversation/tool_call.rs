use super::ids::{ToolCallId, ToolStreamKey};
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
    /// 发起此 tool call 的 provider id（如 `Zhipu`）。来自 ToolCallStart 事件。
    pub provider_id: Option<String>,
    /// 发起此 tool call 的 model id（如 `Zhipu/glm-5.2`）。来自 turn context。
    pub model_id: Option<String>,
    /// 发起此 tool call 的 role（main / subagent / 角色名）。主 turn 为 None。
    pub role: Option<String>,
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
            provider_id: None,
            model_id: None,
            role: None,
        }
    }
    pub fn update_args(&mut self, partial_args: impl Into<String>) {
        self.args_preview = partial_args.into();
    }

    pub fn update(
        &mut self,
        arguments: Option<String>,
        status: ToolCallStatus,
    ) -> Vec<ToolCallChange> {
        if let Some(arguments) = arguments {
            self.args_preview = arguments;
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
}

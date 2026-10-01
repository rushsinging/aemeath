use super::ids::{ToolCallId, ToolStreamKey};
use super::streaming_preview::ToolStreamingPreviewBuffer;
use super::tool_result_payload::ToolResultPayload;
use crate::tui::model::conversation::agent_activity::AgentActivityLine;

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
    pub activities: Vec<AgentActivityLine>,
    pub streaming_preview: Option<ToolStreamingPreviewBuffer>,
    /// Agent 工具特化元数据（issue #499）。仅 `tool_name == "Agent"` 时由
    /// `Started activity` 事件填充，用于 header 渲染
    /// `Agent - [role] - Provider/model`。prompt 不在此处重复存储，
    /// 渲染时从 `args_preview` 取（已在 ToolCallUpdate status=Ready 时填充）。
    pub agent_meta: Option<AgentMeta>,
}

/// Agent 工具的元数据（issue #499）。
/// 由 runtime 的 `Started activity` 事件携带，
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
    pub fn complete(&mut self, result: ToolResultPayload) {
        let is_error = result.is_error;
        self.result = Some(result);
        self.status = if is_error {
            ToolCallStatus::Error
        } else {
            ToolCallStatus::Success
        };
    }

    pub fn cancel(&mut self) -> bool {
        if matches!(
            self.status,
            ToolCallStatus::Success | ToolCallStatus::Error | ToolCallStatus::Cancelled
        ) {
            return false;
        }
        self.status = ToolCallStatus::Cancelled;
        true
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolCallChange {
    Bound,
    Running,
}

#[cfg(test)]
#[path = "tool_call_tests.rs"]
mod tests;

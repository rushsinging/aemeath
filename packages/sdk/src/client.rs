//! AgentClient trait — Agent Runtime 对外的统一接口。

use async_trait::async_trait;

use crate::{
    CancelRunOutcome, ChatRequest, ChatStream, ConfigUpdate, ConfigUpdateResult, ConfigView, RunId,
};

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

/// Agent Runtime 的统一客户端 trait。
///
/// #567 后 trait 只有 `chat()`——所有交互通过事件流：
/// - **写操作** → `ChatInputEvent`（push_input_event → gate → loop idle 分支执行）
/// - **结果回传** → `ChatEvent` 流（事件驱动，TUI 监听）
#[async_trait]
pub trait AgentClient: Send + Sync + 'static {
    /// 同步、幂等地打断指定 Run。返回前 Runtime 已进入 Cancelling 并触发 cancellation scope。
    fn cancel_run(&self, run_id: &RunId) -> CancelRunOutcome;

    /// 完成 Runtime-owned interaction waiter。
    fn reply_interaction(
        &self,
        _request_id: &crate::InteractionRequestId,
        _reply: crate::InteractionReply,
    ) -> crate::InteractionCommandOutcome {
        crate::InteractionCommandOutcome::NotFound
    }

    /// 取消 Runtime-owned interaction waiter。
    fn cancel_interaction(
        &self,
        _request_id: &crate::InteractionRequestId,
        _reason: crate::InteractionCancelReason,
    ) -> crate::InteractionCommandOutcome {
        crate::InteractionCommandOutcome::NotFound
    }

    /// 查询当前已提交配置的 SDK 投影。
    async fn config_view(&self) -> Result<ConfigView, super::SdkError> {
        Err(super::SdkError::Internal(
            "config query is unavailable for this client".to_string(),
        ))
    }

    /// 提交类型化配置更新；返回完整已提交投影。
    async fn update_config(
        &self,
        _update: ConfigUpdate,
    ) -> Result<ConfigUpdateResult, super::SdkError> {
        Err(super::SdkError::Internal(
            "config update is unavailable for this client".to_string(),
        ))
    }

    /// 发起一次 Chat，返回事件流。
    ///
    /// TUI 通过 `ChatRequest.input_events` 发送 `ChatInputEvent`，
    /// 通过 `ChatStream`（`ChatEvent` 流）接收结果。
    async fn chat(&self, input: ChatRequest) -> Result<ChatStream, super::SdkError>;
}

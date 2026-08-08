//! AgentClient trait — Agent Runtime 对外的统一接口。

use async_trait::async_trait;

use crate::{
    CancelCurrentRunOutcome, CancelRunStepOutcome, ChatRequest, ChatStream, ConfigUpdate,
    ConfigUpdateResult, ConfigView, ControlDeadline, RunId, RunStepId, RunTerminationReason,
    TerminateRunOutcome,
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
    /// 同步、幂等地取消当前 Main Run 的当前 Step。
    ///
    /// 调用方无需观察或缓存 Run identity；当前 Main Run 的选择由 Runtime 控制面负责。
    fn cancel_current_run(&self, _deadline: ControlDeadline) -> CancelCurrentRunOutcome {
        CancelCurrentRunOutcome::NoActiveRun
    }

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
    /// TUI 通过 `ChatRequest.ingress` 发送 `ChatInputEvent`，
    /// 通过 `ChatStream`（`ChatEvent` 流）接收结果。
    async fn chat(&self, input: ChatRequest) -> Result<ChatStream, super::SdkError>;
}

#[async_trait]
pub trait DisplayHistoryQuery: Send + Sync + 'static {
    async fn load_display_history_window(
        &self,
        request: crate::DisplayHistoryWindowRequest,
    ) -> Result<crate::DisplayHistoryWindow, super::SdkError>;
}

/// 面向 Server、Coordinator 等管理端的可寻址 Run 控制面。
///
/// 普通交互客户端不应观察或缓存 Run/Step identity；只有确实管理多个 Run 的
/// 调用方才依赖此端口。
pub trait RunControlClient: Send + Sync + 'static {
    fn cancel_run_step(
        &self,
        run_id: &RunId,
        step_id: Option<&RunStepId>,
        deadline: ControlDeadline,
    ) -> CancelRunStepOutcome;

    fn terminate_run(
        &self,
        run_id: &RunId,
        reason: RunTerminationReason,
        deadline: ControlDeadline,
    ) -> TerminateRunOutcome;
}

use uuid::Uuid;

use super::step::{ModelInvocation, RunToolCall, ToolCallStatus};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunStepId(Uuid);

impl RunStepId {
    pub fn new_v7() -> Self {
        Self(Uuid::now_v7())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Created,
    PreparingContext,
    InvokingModel,
    ApplyingResponse,
    AwaitingToolApproval,
    ExecutingTools,
    AwaitingUser,
    Compacting,
    Finishing,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStepStatus {
    Invoking,
    Applying,
    ToolPhase,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct RunStep {
    pub(super) id: RunStepId,
    pub(super) status: RunStepStatus,
    pub(super) invocation: Option<ModelInvocation>,
    pub(super) tool_calls: Vec<RunToolCall>,
}

impl RunStep {
    pub(super) fn is_active(&self) -> bool {
        !matches!(
            self.status,
            RunStepStatus::Done | RunStepStatus::Failed | RunStepStatus::Cancelled
        )
    }

    pub(super) fn is_complete(&self) -> bool {
        self.invocation.is_some()
            && self.tool_calls.iter().all(|call| {
                matches!(
                    call.status(),
                    ToolCallStatus::Success | ToolCallStatus::Error | ToolCallStatus::Cancelled
                )
            })
    }

    pub fn id(&self) -> &RunStepId {
        &self.id
    }

    pub fn status(&self) -> RunStepStatus {
        self.status
    }

    pub fn invocation(&self) -> Option<&ModelInvocation> {
        self.invocation.as_ref()
    }

    pub fn tool_calls(&self) -> &[RunToolCall] {
        &self.tool_calls
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunTransition {
    Start,
    BeginCompaction,
    CompactionCompleted,
    ContextPrepared,
    RetryModel,
    ModelContextExceeded,
    ModelInvoked,
    ResponseWithTools,
    ResponseWithoutTools,
    ContinueAfterResponse,
    ToolsApproved,
    AwaitUser,
    UserResumed,
    ToolsCompleted,
    Finish,
    CancellationFinished,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunTransitionError {
    #[error("非法 Run 状态迁移：{from:?} --{transition:?}-->")]
    IllegalTransition {
        from: RunStatus,
        transition: RunTransition,
    },
    #[error("Run 当前不是活动状态：{0:?}")]
    RunNotActive(RunStatus),
    #[error("未找到 Run Step")]
    StepNotFound,
    #[error("Run Step 当前不是活动状态")]
    StepNotActive,
    #[error("Run 已存在活动 Step")]
    ActiveStepAlreadyExists,
    #[error("Run Step 尚未完整收口")]
    StepIncomplete,
    #[error("Run Step 已记录 Model Invocation")]
    InvocationAlreadyRecorded,
    #[error("未找到 Tool Call")]
    ToolCallNotFound,
    #[error("非法 Tool Call 状态迁移：{from:?} --> {to:?}")]
    IllegalToolCallTransition {
        from: ToolCallStatus,
        to: ToolCallStatus,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunCancellationRequest {
    Accepted,
    AlreadyCancelling,
    AlreadyTerminal,
}

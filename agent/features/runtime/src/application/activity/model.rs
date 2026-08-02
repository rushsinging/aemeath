use sdk::{
    ActivityAudienceView, ActivityDetailView, ActivityId, ActivityKindView, ActivitySourceView,
    ActivityStateView, ActivityTimingView, ActivityView, RunId, RunStepId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActivitySource {
    Run,
    RunStep(RunStepId),
    ModelInvocation(sdk::ModelInvocationId),
    ToolCall(sdk::ToolCallId),
    HookDispatch(ActivityId),
    Interaction(sdk::InteractionRequestId),
    ChildRun(RunId),
}

impl ActivitySource {
    fn to_sdk(&self) -> ActivitySourceView {
        match self {
            Self::Run => ActivitySourceView::Run,
            Self::RunStep(id) => ActivitySourceView::RunStep(id.clone()),
            Self::ModelInvocation(id) => ActivitySourceView::ModelInvocation(id.clone()),
            Self::ToolCall(id) => ActivitySourceView::ToolCall(id.clone()),
            Self::HookDispatch(id) => ActivitySourceView::HookDispatch(id.clone()),
            Self::Interaction(id) => ActivitySourceView::Interaction(id.clone()),
            Self::ChildRun(id) => ActivitySourceView::ChildRun(id.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunPhaseKind {
    DrainingInput,
    PreparingContext,
    ApplyingResponse,
    AwaitingToolApproval,
    ExecutingTools,
    FinalizingStep,
    CancellingStep,
    Terminating,
}

impl RunPhaseKind {
    fn to_sdk(self) -> sdk::RunPhaseKindView {
        match self {
            Self::DrainingInput => sdk::RunPhaseKindView::DrainingInput,
            Self::PreparingContext => sdk::RunPhaseKindView::PreparingContext,
            Self::ApplyingResponse => sdk::RunPhaseKindView::ApplyingResponse,
            Self::AwaitingToolApproval => sdk::RunPhaseKindView::AwaitingToolApproval,
            Self::ExecutingTools => sdk::RunPhaseKindView::ExecutingTools,
            Self::FinalizingStep => sdk::RunPhaseKindView::FinalizingStep,
            Self::CancellingStep => sdk::RunPhaseKindView::CancellingStep,
            Self::Terminating => sdk::RunPhaseKindView::Terminating,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActivityKind {
    Run,
    RunPhase(RunPhaseKind),
    ModelInvocation,
    ToolCall,
    HookDispatch,
    Compaction,
    Interaction,
    ChildRun,
}

impl ActivityKind {
    fn to_sdk(&self) -> ActivityKindView {
        match self {
            Self::Run => ActivityKindView::Run,
            Self::RunPhase(kind) => ActivityKindView::RunPhase(kind.to_sdk()),
            Self::ModelInvocation => ActivityKindView::ModelInvocation,
            Self::ToolCall => ActivityKindView::ToolCall,
            Self::HookDispatch => ActivityKindView::HookDispatch,
            Self::Compaction => ActivityKindView::Compaction,
            Self::Interaction => ActivityKindView::Interaction,
            Self::ChildRun => ActivityKindView::ChildRun,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActivityDetail {
    Run,
    Phase(RunPhaseKind),
    Model {
        model: String,
        attempt: u32,
        stream: sdk::ModelStreamStateView,
    },
    Tool {
        name: String,
        summary: Option<String>,
        parallel_count: u16,
    },
    Hook {
        point: sdk::HookPointView,
        script: String,
        attempt: u8,
    },
    Compact {
        stage: sdk::CompactStageView,
        current: Option<u32>,
        total: Option<u32>,
    },
    Interaction {
        kind: sdk::InteractionKindView,
    },
    ChildRun {
        role: String,
        model: String,
    },
}

impl ActivityDetail {
    fn to_sdk(&self) -> ActivityDetailView {
        match self {
            Self::Run => ActivityDetailView::Run {
                purpose: sdk::RunPurposeView::Main,
            },
            Self::Phase(phase) => ActivityDetailView::Phase {
                phase: phase.to_sdk(),
            },
            Self::Model {
                model,
                attempt,
                stream,
            } => ActivityDetailView::Model {
                model: model.clone(),
                attempt: *attempt,
                stream: *stream,
            },
            Self::Tool {
                name,
                summary,
                parallel_count,
            } => ActivityDetailView::Tool {
                name: name.clone(),
                summary: summary.clone(),
                parallel_count: *parallel_count,
            },
            Self::Hook {
                point,
                script,
                attempt,
            } => ActivityDetailView::Hook {
                point: *point,
                script: script.clone(),
                attempt: *attempt,
            },
            Self::Compact {
                stage,
                current,
                total,
            } => ActivityDetailView::Compact {
                stage: *stage,
                current: *current,
                total: *total,
            },
            Self::Interaction { kind } => ActivityDetailView::Interaction { kind: *kind },
            Self::ChildRun { role, model } => ActivityDetailView::ChildRun {
                role: role.clone(),
                model: model.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivityState {
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
    Terminated,
}

impl ActivityState {
    fn to_sdk(self) -> ActivityStateView {
        match self {
            Self::Running => ActivityStateView::Running,
            Self::Waiting => ActivityStateView::Waiting,
            Self::Succeeded => ActivityStateView::Succeeded,
            Self::Failed => ActivityStateView::Failed,
            Self::Cancelled => ActivityStateView::Cancelled,
            Self::Terminated => ActivityStateView::Terminated,
        }
    }

    pub(crate) fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Terminated
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActivityTiming {
    pub(crate) total_elapsed_ms: u64,
    pub(crate) active_elapsed_ms: u64,
    pub(crate) state_elapsed_ms: u64,
    pub(crate) started_at_unix_ms: Option<u64>,
    pub(crate) finished_at_unix_ms: Option<u64>,
}

impl ActivityTiming {
    fn to_sdk(self) -> ActivityTimingView {
        ActivityTimingView {
            total_elapsed_ms: self.total_elapsed_ms,
            active_elapsed_ms: self.active_elapsed_ms,
            state_elapsed_ms: self.state_elapsed_ms,
            started_at_unix_ms: self.started_at_unix_ms,
            finished_at_unix_ms: self.finished_at_unix_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ActivityObservation {
    pub(crate) id: ActivityId,
    pub(crate) run_id: RunId,
    pub(crate) run_step_id: Option<RunStepId>,
    pub(crate) parent_activity_id: Option<ActivityId>,
    pub(crate) source: ActivitySource,
    pub(crate) kind: ActivityKind,
    pub(crate) state: ActivityState,
    pub(crate) detail: ActivityDetail,
    pub(crate) audience: ActivityAudienceView,
    pub(crate) revision: u64,
    pub(crate) timing: ActivityTiming,
    pub(crate) started_at_monotonic_ms: u64,
    pub(crate) last_transition_monotonic_ms: u64,
    pub(crate) active_started_monotonic_ms: Option<u64>,
}

impl ActivityObservation {
    pub(crate) fn to_sdk(&self, now_monotonic_ms: u64) -> ActivityView {
        let timing = if self.state.is_terminal() {
            self.timing
        } else {
            let total = now_monotonic_ms.saturating_sub(self.started_at_monotonic_ms);
            let active = self.timing.active_elapsed_ms
                + self
                    .active_started_monotonic_ms
                    .map(|started| now_monotonic_ms.saturating_sub(started))
                    .unwrap_or_default();
            ActivityTiming {
                total_elapsed_ms: total,
                active_elapsed_ms: active,
                state_elapsed_ms: now_monotonic_ms
                    .saturating_sub(self.last_transition_monotonic_ms),
                ..self.timing
            }
        };
        ActivityView {
            id: self.id.clone(),
            run_id: self.run_id.clone(),
            run_step_id: self.run_step_id.clone(),
            parent_activity_id: self.parent_activity_id.clone(),
            source: self.source.to_sdk(),
            kind: self.kind.to_sdk(),
            state: self.state.to_sdk(),
            detail: self.detail.to_sdk(),
            audience: self.audience,
            revision: self.revision,
            timing: timing.to_sdk(),
        }
    }
}

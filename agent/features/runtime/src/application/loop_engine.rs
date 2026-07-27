mod engine;
pub(crate) mod event_strategy;
mod input;
pub(crate) mod input_strategy;
pub(crate) mod llm_log;
pub(crate) mod llm_strategy;
pub(crate) mod shared;
mod stuck_guard;
pub(crate) mod tool_strategy;

pub(crate) use engine::fail_run;
pub use engine::{
    execute_prepared_loop, run_loop, ApprovalRequiredCall, CompactionPort, DrainEpoch,
    DrainOutcome, EventSinkPort, ExecutionStatePort, InputPort, InteractionMailboxPort,
    InteractionWorkOutcome, InternalContinuationKind, LoopDirective, LoopEngineError,
    LoopEnginePort, LoopInput, ModelInvocationPort, ModelStep, PendingInteractionItem,
    PendingInteractionWork, PlanApprovalPort, RunControlPort, RunLifecyclePort,
    StepPersistencePort, StepTokenUsage, StopHookPort, StuckHandlingPort, SuspendedQuestion,
    SuspendedToolCall, ToolGuardDecision, ToolOrchestrationPort, ToolStep,
};
pub use input::{split_input_events, RuntimeControl, RuntimeInputBatch, UserRunInput};
pub use stuck_guard::{StuckDecision, StuckGuard};

#[cfg(test)]
mod tests;

pub mod chat;
pub(crate) mod compaction;
pub(crate) mod context_request;
mod engine;
pub(crate) mod event_strategy;
mod input;
pub(crate) mod input_strategy;
pub(crate) mod llm_log;
pub(crate) mod llm_strategy;
pub(crate) mod run_finalization;
pub(crate) mod run_lifecycle;
pub(crate) mod shared;
pub(crate) mod step_persistence;
mod stuck_guard;
pub(crate) mod tool_strategy;

pub(crate) use engine::fail_run;
pub use engine::{
    execute_prepared_loop, run_loop, ApprovalRequiredCall, CompactionPort, DrainEpoch,
    DrainOutcome, EventSinkPort, InputPort, InteractionMailboxPort, InteractionWorkOutcome,
    InternalContinuationKind, LoopCapabilityAdapter, LoopDirective, LoopEngineError, LoopInput,
    ModelInvocationPort, ModelStep, PendingInteractionItem, PendingInteractionWork,
    PlanApprovalPort, RunControlPort, RunLifecyclePort, StepCommit, StepPersistencePort,
    StepTokenUsage, StuckHandlingPort, SuspendedQuestion, SuspendedToolCall, ToolGuardDecision,
    ToolOrchestrationPort, ToolStep,
};
pub use input::{split_input_events, RuntimeControl, RuntimeInputBatch, UserRunInput};
pub use stuck_guard::{StuckDecision, StuckGuard};

#[cfg(test)]
mod tests;

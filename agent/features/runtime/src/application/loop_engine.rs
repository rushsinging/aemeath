pub mod chat;
pub(crate) mod compaction;
pub(crate) mod context_request;
mod engine;
pub(crate) mod event_strategy;
pub(crate) mod input_strategy;
pub(crate) mod llm_log;
pub(crate) mod llm_strategy;
pub(crate) mod run_finalization;
mod run_loop;
pub(crate) mod run_ports;
pub(crate) mod run_services;
pub(crate) mod shared;
pub(crate) mod step_persistence;
mod stuck_guard;
pub(crate) mod tool_strategy;

pub(crate) use engine::fail_run;
#[cfg(test)]
use engine::run_loop;
pub use engine::{
    execute_prepared_loop, ApprovalRequiredCall, CompactionPort, DrainEpoch, DrainOutcome,
    EventSinkPort, InputPort, InteractionMailboxPort, InteractionWorkOutcome,
    InternalContinuationKind, LoopDirective, LoopEngineError, LoopInput, ModelInvocationPort,
    ModelStep, PendingInteractionItem, PendingInteractionWork, PlanApprovalPort, RunControlPort,
    RunLifecyclePort, StepCommit, StepPersistencePort, StepTokenUsage, StuckHandlingPort,
    SuspendedQuestion, SuspendedToolCall, ToolGuardDecision, ToolOrchestrationPort, ToolStep,
};
pub use run_loop::RunLoop;
pub use stuck_guard::{StuckDecision, StuckGuard};

#[cfg(test)]
mod tests;

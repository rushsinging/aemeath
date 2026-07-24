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
    run_loop, DrainEpoch, DrainOutcome, InternalContinuationKind, LoopDirective, LoopEngineError,
    LoopInput, ModelStep, RunLoopPort, StepTokenUsage, ToolGuardDecision, ToolStep,
};
pub use input::{split_input_events, RuntimeControl, RuntimeInputBatch, UserRunInput};
pub use stuck_guard::{StuckDecision, StuckGuard};

#[cfg(test)]
mod tests;

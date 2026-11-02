mod execution;
#[cfg(test)]
pub(crate) mod fakes;
mod response;
mod task;

pub use execution::CompleteReflectionResult;
#[cfg_attr(not(test), allow(unused_imports))]
pub use task::{
    DetachedCompletionHook, MemoryUpdateNotice, ReflectionRunOutcome, ReflectionTaskAdapter,
    ReflectionTaskCompletion, ReflectionTaskCompletionStatus, ReflectionTaskMetadata,
    ReflectionTaskRequest, ReflectionTaskTrigger,
};

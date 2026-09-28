mod execution;
mod task;
#[cfg(test)]
pub(crate) mod test_support;

pub use execution::CompleteReflectionResult;
#[cfg_attr(not(test), allow(unused_imports))]
pub use task::{
    MemoryUpdateNotice, ReflectionRunOutcome, ReflectionTaskAdapter, ReflectionTaskCompletion,
    ReflectionTaskCompletionStatus, ReflectionTaskMetadata, ReflectionTaskRequest,
    ReflectionTaskTrigger,
};

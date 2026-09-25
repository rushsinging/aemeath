mod execution;
mod task;

pub use execution::{CompleteReflectionResult, ReflectionExecutionError as ReflectionError};
#[cfg_attr(not(test), allow(unused_imports))]
pub use task::{
    ReflectionTaskAdapter, ReflectionTaskCompletionStatus, ReflectionTaskRequest,
    ReflectionTaskSubmitOutcome, ReflectionTaskTrigger,
};

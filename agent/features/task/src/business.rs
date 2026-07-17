mod lifecycle;
mod model;
mod query;
mod state;

#[cfg(test)]
mod query_tests;

pub use lifecycle::{
    detect_batch_all_completed, detect_interrupted_batch, detect_stale_batches,
    InterruptedBatchInfo, StaleBatchInfo,
};
pub use model::{
    Batch, BatchCreateSpec, BatchId, BatchStatus, Task, TaskCommandError, TaskCommandResult,
    TaskCreateSpec, TaskEvent, TaskId, TaskPriority, TaskRevision, TaskStatus,
};
pub use query::{
    TaskLifecycleSnapshot, TaskPriorityStats, TaskReminderItem, TaskReminderSnapshot,
    TaskStoreStats,
};
/// 聚合内部事务状态：仅 crate 内 `TaskStore` backing 可见，**NEVER** 进入
/// 公开 façade（否则消费方可绕过窄端口直接改状态 / 触碰内部 map / counter）。
pub(crate) use state::TaskStoreState;

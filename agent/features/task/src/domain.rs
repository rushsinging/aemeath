mod lifecycle;
mod model;
mod query;
mod snapshot;
mod state;
mod task_access;
mod task_persist;

#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod snapshot_validation_tests;

pub use lifecycle::{
    detect_batch_all_completed, detect_interrupted_batch, detect_stale_batches,
    InterruptedBatchInfoData, StaleBatchInfoData,
};
pub(crate) use model::{TaskCommandError, TaskSnapshotFields};

pub use model::{
    BatchCreateSpecData, BatchData, BatchIdData, BatchStatusData, TaskCommandResultData,
    TaskCreateSpecData, TaskData, TaskEventData, TaskIdData, TaskPriorityData, TaskRevisionData,
    TaskStatusData, TaskViewData,
};
pub use query::{
    TaskBatchSnapshotData, TaskLifecycleSnapshotData, TaskPriorityStatsData, TaskProgressItemData,
    TaskProgressSnapshotData, TaskStoreStatsData,
};
pub use snapshot::{PreparedTaskRestoreData, TaskSnapshotData};
/// 聚合内部事务状态：仅 crate 内 `TaskStore` backing 可见，**NEVER** 进入
/// 公开 façade（否则消费方可绕过窄端口直接改状态 / 触碰内部 map / counter）。
pub(crate) use state::TaskStoreState;
pub use task_access::TaskAccess;
pub use task_persist::TaskPersist;

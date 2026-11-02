use crate::domain::{
    BatchCreateSpecData, BatchData, BatchIdData, TaskBatchSnapshotData, TaskCommandResultData,
    TaskCreateSpecData, TaskData, TaskIdData, TaskLifecycleSnapshotData, TaskPriorityData,
    TaskProgressSnapshotData, TaskRevisionData, TaskStatusData,
};

/// Narrow, TaskData-owned capability for typed TaskData commands and queries.
///
/// This port deliberately exposes neither the backing store nor generic mutation
/// hooks. All methods are synchronous because the in-memory transaction contains
/// no I/O; implementations must release any state guard before returning.
///
/// 只发布 crate 外实际消费的 19 个命令/查询。Batch 生命周期细粒度命令
/// （pause/resume/record_turn）、非 progress 的 transition、单条依赖增删、
/// tag 增删、全局 batch 列表、blocked/cycle 探测与全局 stats 属于 Task-internal
/// 能力，未发布到本 port；crate 内消费者经 [`crate::TaskStore`] 的
/// `pub(crate)` 固有方法访问。
pub trait TaskAccess: Send + Sync {
    fn revision(&self) -> TaskRevisionData;

    /// Atomically clears the complete aggregate. A non-empty clear emits one
    /// `TaskStoreCleared` event and advances revision once; an empty clear is a
    /// no-op with no event/revision.
    fn clear(&self) -> Result<TaskCommandResultData<()>, share::error::DomainError>;

    fn create_batch(
        &self,
        spec: BatchCreateSpecData,
        timestamp: u64,
    ) -> Result<TaskCommandResultData<BatchData>, share::error::DomainError>;
    fn archive_batch(
        &self,
        id: BatchIdData,
    ) -> Result<TaskCommandResultData<BatchData>, share::error::DomainError>;

    fn create_task(
        &self,
        spec: TaskCreateSpecData,
        timestamp: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError>;
    fn transition_with_progress(
        &self,
        id: TaskIdData,
        to: TaskStatusData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskProgressSnapshotData>, share::error::DomainError>;
    fn set_subject(
        &self,
        id: TaskIdData,
        subject: String,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError>;
    fn set_description(
        &self,
        id: TaskIdData,
        description: String,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError>;
    fn set_priority(
        &self,
        id: TaskIdData,
        priority: TaskPriorityData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError>;
    fn replace_dependencies(
        &self,
        task_id: TaskIdData,
        blocked_by_ids: Vec<TaskIdData>,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError>;
    fn delete_with_progress(
        &self,
        id: TaskIdData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskProgressSnapshotData>, share::error::DomainError>;
    fn delete(
        &self,
        id: TaskIdData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError>;

    fn get(&self, id: TaskIdData) -> Option<TaskData>;
    fn current_task_by_seq(&self, seq: u64) -> Option<TaskData>;
    fn list(&self) -> Vec<TaskData>;
    fn batch_snapshot(&self, id: BatchIdData) -> Option<TaskBatchSnapshotData>;
    fn list_batch_snapshots(&self) -> Vec<TaskBatchSnapshotData>;
    fn current_batch(&self) -> Option<BatchIdData>;
    fn lifecycle_snapshot(&self, stale_after_silence_turns: u64) -> TaskLifecycleSnapshotData;
}

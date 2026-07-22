use crate::domain::{
    Batch, BatchCreateSpec, BatchId, Task, TaskCommandError, TaskCommandResult, TaskCreateSpec,
    TaskId, TaskLifecycleSnapshot, TaskPriority, TaskReminderSnapshot, TaskRevision, TaskStatus,
    TaskStoreStats,
};

/// Narrow, Task-owned capability for typed Task commands and queries.
///
/// This port deliberately exposes neither the backing store nor generic mutation
/// hooks. All methods are synchronous because the in-memory transaction contains
/// no I/O; implementations must release any state guard before returning.
pub trait TaskAccess: Send + Sync {
    fn revision(&self) -> TaskRevision;

    /// Atomically clears the complete aggregate. A non-empty clear emits one
    /// `TaskStoreCleared` event and advances revision once; an empty clear is a
    /// no-op with no event/revision.
    fn clear(&self) -> Result<TaskCommandResult<()>, TaskCommandError>;

    fn create_batch(
        &self,
        spec: BatchCreateSpec,
        timestamp: u64,
    ) -> Result<TaskCommandResult<Batch>, TaskCommandError>;
    fn pause_batch(&self, id: BatchId) -> Result<TaskCommandResult<Batch>, TaskCommandError>;
    fn resume_batch(&self, id: BatchId) -> Result<TaskCommandResult<Batch>, TaskCommandError>;
    fn archive_batch(&self, id: BatchId) -> Result<TaskCommandResult<Batch>, TaskCommandError>;
    fn record_batch_turn(
        &self,
        id: BatchId,
        turn: u64,
        active: bool,
    ) -> Result<TaskCommandResult<Batch>, TaskCommandError>;

    fn create_task(
        &self,
        spec: TaskCreateSpec,
        timestamp: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    fn transition(
        &self,
        id: TaskId,
        to: TaskStatus,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    fn set_subject(
        &self,
        id: TaskId,
        subject: String,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    fn set_description(
        &self,
        id: TaskId,
        description: String,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    fn set_priority(
        &self,
        id: TaskId,
        priority: TaskPriority,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    fn add_dependency(
        &self,
        task_id: TaskId,
        blocked_by_id: TaskId,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    fn remove_dependency(
        &self,
        task_id: TaskId,
        blocked_by_id: TaskId,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    fn add_tag(
        &self,
        id: TaskId,
        tag: String,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    fn remove_tag(
        &self,
        id: TaskId,
        tag: &str,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    fn delete(
        &self,
        id: TaskId,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError>;

    fn get(&self, id: TaskId) -> Option<Task>;
    fn current_task_by_seq(&self, seq: u64) -> Option<Task>;
    fn list(&self) -> Vec<Task>;
    fn list_batches(&self) -> Vec<Batch>;
    fn current_batch(&self) -> Option<BatchId>;
    fn stats(&self) -> TaskStoreStats;
    fn reminder_snapshot(&self) -> TaskReminderSnapshot;
    fn lifecycle_snapshot(&self, stale_after_silence_turns: u64) -> TaskLifecycleSnapshot;
    fn is_blocked(&self, id: TaskId) -> Result<bool, TaskCommandError>;
    fn would_create_cycle(&self, task_id: TaskId, blocked_by_id: TaskId) -> bool;
}

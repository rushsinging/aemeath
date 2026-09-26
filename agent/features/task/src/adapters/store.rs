use std::sync::{Mutex, MutexGuard};

use crate::domain::{
    BatchCreateSpecData, BatchData, BatchIdData, PreparedTaskRestoreData, TaskBatchSnapshotData,
    TaskCommandResultData, TaskCreateSpecData, TaskData, TaskIdData, TaskLifecycleSnapshotData,
    TaskPriorityData, TaskProgressSnapshotData, TaskRevisionData, TaskSnapshotData, TaskStatusData,
    TaskStoreState, TaskStoreStatsData,
};
use crate::{TaskAccess, TaskPersist};

/// TaskData BC 的内存事务 backing。
///
/// 全部可变字段收进一个 [`TaskStoreState`]（其自身即携带权威 [`TaskRevisionData`]），
/// 由一把同步 `Mutex` 统一守护；**NEVER** 为 tasks / batches / counters 分别建锁，
/// 避免锁槽拆分导致 state 与 revision 之间出现不一致的中间态。
///
/// 所有 typed mutation 只把已校验的入参转交给聚合方法，在同一次持锁期间完成
/// validate → modify → commit；只有真正改变了 live state 的命令才递增一次
/// revision，失败命令、只读查询与幂等 no-op 一律保持 revision 与
/// `TaskStoreState` 不变（原子失败：ID / revision 耗尽时不留下任何部分写入）。
/// revision 与命令产生的 `value` / `events` 由聚合内部在同一次提交写入，
/// `TaskStore` 自身不做任何二次拼接或事后查询 revision。
///
/// `TaskStore` 只提供同步 API：不依赖任何异步运行时，锁守卫在方法返回前必然
/// 释放，因而调用方也无法在持锁期间跨越 `.await`。这里实现同步的窄
/// [`TaskAccess`] 端口与 [`TaskPersist`] 持久化端口；capture/prepare/install
/// 内部装配仍保持 crate-private，只经端口对外发布，不做 Runtime / Tool wiring
/// 或 legacy storage integration。
#[derive(Debug, Default)]
pub struct TaskStore {
    state: Mutex<TaskStoreState>,
}

impl TaskStore {
    /// 构造一个空 backing：`TaskStoreState::empty()`，revision 从 `0` 开始。
    pub fn new() -> Self {
        Self {
            state: Mutex::new(TaskStoreState::empty()),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_state(state: TaskStoreState) -> Self {
        Self {
            state: Mutex::new(state),
        }
    }

    /// 测试专用：克隆当前完整聚合状态用于原子性断言，不属于生产 API 面。
    #[cfg(test)]
    pub(crate) fn state_snapshot(&self) -> TaskStoreState {
        self.lock().clone()
    }

    /// Captures one coherent persistence image while holding the aggregate's
    /// single lock. Deleted tombstones and runtime-only reverse indexes are not
    /// persisted. Crate-private plumbing behind the [`TaskPersist`] port.
    pub(crate) fn capture_snapshot(&self) -> TaskSnapshotData {
        self.lock().capture_snapshot()
    }

    /// Installs an already validated candidate with one lock acquisition and
    /// one infallible whole-state assignment. Crate-private plumbing behind the
    /// [`TaskPersist`] port.
    pub(crate) fn install_snapshot(&self, prepared: PreparedTaskRestoreData) {
        *self.lock() = prepared.into_candidate();
    }

    /// 持锁；锁中毒意味着可能曾在事务中途 panic，必须停止而不是继续暴露内部状态。
    fn lock(&self) -> MutexGuard<'_, TaskStoreState> {
        self.state
            .lock()
            .expect("task store mutex poisoned; refusing to expose potentially partial state")
    }
}

impl TaskAccess for TaskStore {
    fn revision(&self) -> TaskRevisionData {
        self.lock().revision()
    }

    fn clear(&self) -> Result<TaskCommandResultData<()>, share::error::DomainError> {
        self.lock().clear()
    }

    fn create_batch(
        &self,
        spec: BatchCreateSpecData,
        timestamp: u64,
    ) -> Result<TaskCommandResultData<BatchData>, share::error::DomainError> {
        self.lock().create_batch(spec, timestamp)
    }

    fn pause_batch(
        &self,
        id: BatchIdData,
    ) -> Result<TaskCommandResultData<BatchData>, share::error::DomainError> {
        self.lock().pause_batch(id)
    }

    fn resume_batch(
        &self,
        id: BatchIdData,
    ) -> Result<TaskCommandResultData<BatchData>, share::error::DomainError> {
        self.lock().resume_batch(id)
    }

    fn archive_batch(
        &self,
        id: BatchIdData,
    ) -> Result<TaskCommandResultData<BatchData>, share::error::DomainError> {
        self.lock().archive_batch(id)
    }

    fn record_batch_turn(
        &self,
        id: BatchIdData,
        turn: u64,
        active: bool,
    ) -> Result<TaskCommandResultData<BatchData>, share::error::DomainError> {
        self.lock().record_batch_turn(id, turn, active)
    }

    fn create_task(
        &self,
        spec: TaskCreateSpecData,
        timestamp: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        self.lock().create_task(spec, timestamp)
    }

    fn transition_with_progress(
        &self,
        id: TaskIdData,
        to: TaskStatusData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskProgressSnapshotData>, share::error::DomainError> {
        self.lock().transition_with_progress(id, to, updated_at)
    }

    fn transition(
        &self,
        id: TaskIdData,
        to: TaskStatusData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        self.lock().transition(id, to, updated_at)
    }

    fn set_subject(
        &self,
        id: TaskIdData,
        subject: String,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        self.lock().set_subject(id, subject, updated_at)
    }

    fn set_description(
        &self,
        id: TaskIdData,
        description: String,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        self.lock().set_description(id, description, updated_at)
    }

    fn set_priority(
        &self,
        id: TaskIdData,
        priority: TaskPriorityData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        self.lock().set_priority(id, priority, updated_at)
    }

    fn add_dependency(
        &self,
        task_id: TaskIdData,
        blocked_by_id: TaskIdData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        self.lock()
            .add_dependency(task_id, blocked_by_id, updated_at)
    }

    fn replace_dependencies(
        &self,
        task_id: TaskIdData,
        blocked_by_ids: Vec<TaskIdData>,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        self.lock()
            .replace_dependencies(task_id, blocked_by_ids, updated_at)
    }

    fn remove_dependency(
        &self,
        task_id: TaskIdData,
        blocked_by_id: TaskIdData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        self.lock()
            .remove_dependency(task_id, blocked_by_id, updated_at)
    }

    fn add_tag(
        &self,
        id: TaskIdData,
        tag: String,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        self.lock().add_tag(id, tag, updated_at)
    }

    fn remove_tag(
        &self,
        id: TaskIdData,
        tag: &str,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        self.lock().remove_tag(id, tag, updated_at)
    }

    fn delete_with_progress(
        &self,
        id: TaskIdData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskProgressSnapshotData>, share::error::DomainError> {
        self.lock().delete_with_progress(id, updated_at)
    }

    fn delete(
        &self,
        id: TaskIdData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        self.lock().delete(id, updated_at)
    }

    fn get(&self, id: TaskIdData) -> Option<TaskData> {
        self.lock().get(id)
    }

    fn current_task_by_seq(&self, seq: u64) -> Option<TaskData> {
        self.lock().current_task_by_seq(seq)
    }

    fn list(&self) -> Vec<TaskData> {
        self.lock().list()
    }

    fn list_batches(&self) -> Vec<BatchData> {
        self.lock().list_batches()
    }

    fn batch_snapshot(&self, id: BatchIdData) -> Option<TaskBatchSnapshotData> {
        self.lock().batch_snapshot(id)
    }

    fn list_batch_snapshots(&self) -> Vec<TaskBatchSnapshotData> {
        self.lock().list_batch_snapshots()
    }

    fn current_batch(&self) -> Option<BatchIdData> {
        self.lock().current_batch()
    }

    fn stats(&self) -> TaskStoreStatsData {
        self.lock().stats()
    }

    fn lifecycle_snapshot(&self, stale_after_silence_turns: u64) -> TaskLifecycleSnapshotData {
        self.lock().lifecycle_snapshot(stale_after_silence_turns)
    }

    fn is_blocked(&self, id: TaskIdData) -> Result<bool, share::error::DomainError> {
        self.lock().is_blocked(id)
    }

    fn would_create_cycle(&self, task_id: TaskIdData, blocked_by_id: TaskIdData) -> bool {
        self.lock().would_create_cycle(task_id, blocked_by_id)
    }
}

impl TaskPersist for TaskStore {
    fn collect_snapshot(&self) -> TaskSnapshotData {
        self.capture_snapshot()
    }

    fn prepare_restore(
        &self,
        snapshot: &TaskSnapshotData,
    ) -> Result<PreparedTaskRestoreData, share::error::DomainError> {
        // Clone before validating: the candidate is built from the caller's
        // snapshot alone and never reads or mutates the live backing, so a
        // rejected restore leaves both the argument and this store untouched.
        snapshot.clone().prepare()
    }

    fn commit_restore(&self, token: PreparedTaskRestoreData) {
        self.install_snapshot(token);
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

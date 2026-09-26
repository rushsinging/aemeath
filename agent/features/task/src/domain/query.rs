use super::{
    detect_batch_all_completed, detect_interrupted_batch, detect_stale_batches, BatchData,
    BatchIdData, InterruptedBatchInfoData, StaleBatchInfoData, TaskData, TaskIdData,
    TaskPriorityData, TaskStatusData, TaskStoreState,
};

/// Counts grouped by the closed TaskData priority vocabulary.
///
/// A fixed-field value keeps iteration and serialization deterministic and does
/// not expose the store's hash-based representation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskPriorityStatsData {
    pub low: usize,
    pub normal: usize,
    pub high: usize,
    pub urgent: usize,
}

impl TaskPriorityStatsData {
    fn increment(&mut self, priority: TaskPriorityData) {
        let count = match priority {
            TaskPriorityData::Low => &mut self.low,
            TaskPriorityData::Normal => &mut self.normal,
            TaskPriorityData::High => &mut self.high,
            TaskPriorityData::Urgent => &mut self.urgent,
        };
        *count += 1;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskBatchSnapshotData {
    batch: BatchData,
    stats: TaskStoreStatsData,
    tasks: Vec<TaskData>,
}

impl TaskBatchSnapshotData {
    pub fn batch(&self) -> &BatchData {
        &self.batch
    }

    pub const fn stats(&self) -> TaskStoreStatsData {
        self.stats
    }

    pub fn tasks(&self) -> &[TaskData] {
        &self.tasks
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskStoreStatsData {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub deleted: usize,
    /// Priority counts include live Tasks only.
    pub by_priority: TaskPriorityStatsData,
}

impl TaskStoreStatsData {
    /// 从单个 task 计数（读模型构造用）。
    fn increment(&mut self, task: &TaskData) {
        self.total += 1;
        match task.status() {
            TaskStatusData::Pending => self.pending += 1,
            TaskStatusData::InProgress => self.in_progress += 1,
            TaskStatusData::Completed => self.completed += 1,
            TaskStatusData::Deleted => {
                self.deleted += 1;
                return;
            }
        }
        self.by_priority.increment(task.priority());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskProgressItemData {
    pub id: TaskIdData,
    pub seq: u64,
    pub subject: String,
    pub status: TaskStatusData,
    pub completed_at: Option<u64>,
}

impl From<&TaskData> for TaskProgressItemData {
    fn from(task: &TaskData) -> Self {
        Self {
            id: task.id(),
            seq: task.seq(),
            subject: task.subject().to_owned(),
            status: task.status(),
            completed_at: task.completed_at(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskProgressSnapshotData {
    pub task_list_id: BatchIdData,
    pub summary: Option<String>,
    pub task_list_status: super::BatchStatusData,
    pub updated: TaskProgressItemData,
    pub recently_completed: Vec<TaskProgressItemData>,
    pub in_progress: Vec<TaskProgressItemData>,
    pub ready: Vec<TaskProgressItemData>,
    pub ready_omitted: usize,
    pub blocked_count: usize,
    pub auto_closed: bool,
    pub auto_reopened: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLifecycleSnapshotData {
    pub current_batch: Option<BatchIdData>,
    pub stale_after_silence_turns: u64,
    pub all_completed: Option<BatchIdData>,
    pub interrupted: Option<InterruptedBatchInfoData>,
    pub stale_batches: Vec<StaleBatchInfoData>,
}

impl TaskStoreState {
    /// Returns an owned read model, never a handle into mutable store state.
    /// Commands use the tombstone to preserve idempotent delete semantics; list
    /// and lifecycle projections filter it before exposing live work.
    pub fn get(&self, id: TaskIdData) -> Option<TaskData> {
        self.tasks().get(&id).cloned()
    }

    pub fn current_task_by_seq(&self, seq: u64) -> Option<TaskData> {
        let batch = self.current_batch()?;
        self.tasks()
            .values()
            .find(|task| {
                task.batch() == batch
                    && task.seq() == seq
                    && task.status() != TaskStatusData::Deleted
            })
            .cloned()
    }

    /// Returns all live Tasks in ascending typed-ID order.
    pub fn list(&self) -> Vec<TaskData> {
        let mut tasks: Vec<_> = self
            .tasks()
            .values()
            .filter(|task| task.status() != TaskStatusData::Deleted)
            .cloned()
            .collect();
        tasks.sort_unstable_by_key(TaskData::id);
        tasks
    }

    /// Returns all Batches in ascending typed-ID order.
    pub fn list_batches(&self) -> Vec<BatchData> {
        let mut batches: Vec<_> = self.batches().values().cloned().collect();
        batches.sort_unstable_by_key(BatchData::id);
        batches
    }

    /// Returns an owned, coherent read model for one BatchData.
    pub fn batch_snapshot(&self, id: BatchIdData) -> Option<TaskBatchSnapshotData> {
        let batch = self.batches().get(&id)?.clone();
        let mut tasks: Vec<_> = self
            .tasks()
            .values()
            .filter(|task| task.batch() == id && task.status() != TaskStatusData::Deleted)
            .cloned()
            .collect();
        tasks.sort_unstable_by_key(TaskData::id);
        let mut stats = TaskStoreStatsData::default();
        for task in &tasks {
            stats.increment(task);
        }
        Some(TaskBatchSnapshotData {
            batch,
            stats,
            tasks,
        })
    }

    /// Returns all BatchData read models in ascending typed-ID order.
    pub fn list_batch_snapshots(&self) -> Vec<TaskBatchSnapshotData> {
        self.list_batches()
            .into_iter()
            .filter_map(|batch| self.batch_snapshot(batch.id()))
            .collect()
    }

    pub fn stats(&self) -> TaskStoreStatsData {
        self.tasks()
            .values()
            .fold(TaskStoreStatsData::default(), |mut stats, task| {
                stats.total += 1;
                match task.status() {
                    TaskStatusData::Pending => stats.pending += 1,
                    TaskStatusData::InProgress => stats.in_progress += 1,
                    TaskStatusData::Completed => stats.completed += 1,
                    TaskStatusData::Deleted => stats.deleted += 1,
                }
                if task.status() != TaskStatusData::Deleted {
                    stats.by_priority.increment(task.priority());
                }
                stats
            })
    }

    pub fn progress_snapshot(
        &self,
        batch_id: BatchIdData,
        updated_id: TaskIdData,
        auto_closed: bool,
        auto_reopened: bool,
    ) -> Option<TaskProgressSnapshotData> {
        const RECENT_LIMIT: usize = 2;
        const READY_LIMIT: usize = 2;

        let batch = self.batches().get(&batch_id)?;
        let updated = self.tasks().get(&updated_id)?;
        let mut tasks = self
            .tasks()
            .values()
            .filter(|task| task.batch() == batch_id && task.status() != TaskStatusData::Deleted)
            .collect::<Vec<_>>();
        tasks.sort_unstable_by_key(|task| task.id());

        let mut recently_completed = tasks
            .iter()
            .copied()
            .filter(|task| task.status() == TaskStatusData::Completed)
            .collect::<Vec<_>>();
        recently_completed.sort_unstable_by(|left, right| {
            right
                .completed_at()
                .cmp(&left.completed_at())
                .then_with(|| right.id().cmp(&left.id()))
        });
        let recently_completed = recently_completed
            .into_iter()
            .take(RECENT_LIMIT)
            .map(TaskProgressItemData::from)
            .collect();

        let in_progress = tasks
            .iter()
            .copied()
            .filter(|task| task.status() == TaskStatusData::InProgress)
            .map(TaskProgressItemData::from)
            .collect();

        let (ready, blocked): (Vec<_>, Vec<_>) = tasks
            .iter()
            .copied()
            .filter(|task| task.status() == TaskStatusData::Pending)
            .partition(|task| self.blocking_ids(task.id()).is_ok_and(|ids| ids.is_empty()));
        let ready_omitted = ready.len().saturating_sub(READY_LIMIT);
        let ready = ready
            .into_iter()
            .take(READY_LIMIT)
            .map(TaskProgressItemData::from)
            .collect();

        Some(TaskProgressSnapshotData {
            task_list_id: batch_id,
            summary: batch.summary().map(str::to_owned),
            task_list_status: batch.status(),
            updated: TaskProgressItemData::from(updated),
            recently_completed,
            in_progress,
            ready,
            ready_omitted,
            blocked_count: blocked.len(),
            auto_closed,
            auto_reopened,
        })
    }

    /// Composes the existing pure lifecycle detectors over one deterministic
    /// state read. No lifecycle mutation is performed.
    pub fn lifecycle_snapshot(&self, stale_after_silence_turns: u64) -> TaskLifecycleSnapshotData {
        let tasks = self.list();
        let batches = self.list_batches();
        let current_batch = self.current_batch();
        TaskLifecycleSnapshotData {
            current_batch,
            stale_after_silence_turns,
            all_completed: detect_batch_all_completed(current_batch, &tasks),
            interrupted: current_batch
                .and_then(|id| detect_interrupted_batch(id, &tasks, &batches, true)),
            stale_batches: detect_stale_batches(&tasks, &batches, stale_after_silence_turns),
        }
    }
}

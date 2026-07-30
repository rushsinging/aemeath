use super::{
    detect_batch_all_completed, detect_interrupted_batch, detect_stale_batches, Batch, BatchId,
    InterruptedBatchInfo, StaleBatchInfo, Task, TaskId, TaskPriority, TaskStatus, TaskStoreState,
};

/// Counts grouped by the closed Task priority vocabulary.
///
/// A fixed-field value keeps iteration and serialization deterministic and does
/// not expose the store's hash-based representation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskPriorityStats {
    pub low: usize,
    pub normal: usize,
    pub high: usize,
    pub urgent: usize,
}

impl TaskPriorityStats {
    fn increment(&mut self, priority: TaskPriority) {
        let count = match priority {
            TaskPriority::Low => &mut self.low,
            TaskPriority::Normal => &mut self.normal,
            TaskPriority::High => &mut self.high,
            TaskPriority::Urgent => &mut self.urgent,
        };
        *count += 1;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskBatchStats {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub by_priority: TaskPriorityStats,
}

impl TaskBatchStats {
    fn increment(&mut self, task: &Task) {
        self.total += 1;
        match task.status() {
            TaskStatus::Pending => self.pending += 1,
            TaskStatus::InProgress => self.in_progress += 1,
            TaskStatus::Completed => self.completed += 1,
            TaskStatus::Deleted => return,
        }
        self.by_priority.increment(task.priority());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskBatchSnapshot {
    batch: Batch,
    stats: TaskBatchStats,
    tasks: Vec<Task>,
}

impl TaskBatchSnapshot {
    pub fn batch(&self) -> &Batch {
        &self.batch
    }

    pub const fn stats(&self) -> TaskBatchStats {
        self.stats
    }

    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TaskStoreStats {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub deleted: usize,
    /// Priority counts include live Tasks only.
    pub by_priority: TaskPriorityStats,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskProgressItem {
    pub id: TaskId,
    pub seq: u64,
    pub subject: String,
    pub status: TaskStatus,
    pub completed_at: Option<u64>,
}

impl From<&Task> for TaskProgressItem {
    fn from(task: &Task) -> Self {
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
pub struct TaskProgressSnapshot {
    pub task_list_id: BatchId,
    pub summary: Option<String>,
    pub task_list_status: super::BatchStatus,
    pub updated: TaskProgressItem,
    pub recently_completed: Vec<TaskProgressItem>,
    pub in_progress: Vec<TaskProgressItem>,
    pub ready: Vec<TaskProgressItem>,
    pub ready_omitted: usize,
    pub blocked_count: usize,
    pub auto_closed: bool,
    pub auto_reopened: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLifecycleSnapshot {
    pub current_batch: Option<BatchId>,
    pub stale_after_silence_turns: u64,
    pub all_completed: Option<BatchId>,
    pub interrupted: Option<InterruptedBatchInfo>,
    pub stale_batches: Vec<StaleBatchInfo>,
}

impl TaskStoreState {
    /// Returns an owned read model, never a handle into mutable store state.
    /// Commands use the tombstone to preserve idempotent delete semantics; list
    /// and lifecycle projections filter it before exposing live work.
    pub fn get(&self, id: TaskId) -> Option<Task> {
        self.tasks().get(&id).cloned()
    }

    pub fn current_task_by_seq(&self, seq: u64) -> Option<Task> {
        let batch = self.current_batch()?;
        self.tasks()
            .values()
            .find(|task| {
                task.batch() == batch && task.seq() == seq && task.status() != TaskStatus::Deleted
            })
            .cloned()
    }

    /// Returns all live Tasks in ascending typed-ID order.
    pub fn list(&self) -> Vec<Task> {
        let mut tasks: Vec<_> = self
            .tasks()
            .values()
            .filter(|task| task.status() != TaskStatus::Deleted)
            .cloned()
            .collect();
        tasks.sort_unstable_by_key(Task::id);
        tasks
    }

    /// Returns all Batches in ascending typed-ID order.
    pub fn list_batches(&self) -> Vec<Batch> {
        let mut batches: Vec<_> = self.batches().values().cloned().collect();
        batches.sort_unstable_by_key(Batch::id);
        batches
    }

    /// Returns an owned, coherent read model for one Batch.
    pub fn batch_snapshot(&self, id: BatchId) -> Option<TaskBatchSnapshot> {
        let batch = self.batches().get(&id)?.clone();
        let mut tasks: Vec<_> = self
            .tasks()
            .values()
            .filter(|task| task.batch() == id && task.status() != TaskStatus::Deleted)
            .cloned()
            .collect();
        tasks.sort_unstable_by_key(Task::id);
        let mut stats = TaskBatchStats::default();
        for task in &tasks {
            stats.increment(task);
        }
        Some(TaskBatchSnapshot {
            batch,
            stats,
            tasks,
        })
    }

    /// Returns all Batch read models in ascending typed-ID order.
    pub fn list_batch_snapshots(&self) -> Vec<TaskBatchSnapshot> {
        self.list_batches()
            .into_iter()
            .filter_map(|batch| self.batch_snapshot(batch.id()))
            .collect()
    }

    pub fn stats(&self) -> TaskStoreStats {
        self.tasks()
            .values()
            .fold(TaskStoreStats::default(), |mut stats, task| {
                stats.total += 1;
                match task.status() {
                    TaskStatus::Pending => stats.pending += 1,
                    TaskStatus::InProgress => stats.in_progress += 1,
                    TaskStatus::Completed => stats.completed += 1,
                    TaskStatus::Deleted => stats.deleted += 1,
                }
                if task.status() != TaskStatus::Deleted {
                    stats.by_priority.increment(task.priority());
                }
                stats
            })
    }

    pub fn progress_snapshot(
        &self,
        batch_id: BatchId,
        updated_id: TaskId,
        auto_closed: bool,
        auto_reopened: bool,
    ) -> Option<TaskProgressSnapshot> {
        const RECENT_LIMIT: usize = 2;
        const READY_LIMIT: usize = 2;

        let batch = self.batches().get(&batch_id)?;
        let updated = self.tasks().get(&updated_id)?;
        let mut tasks = self
            .tasks()
            .values()
            .filter(|task| task.batch() == batch_id && task.status() != TaskStatus::Deleted)
            .collect::<Vec<_>>();
        tasks.sort_unstable_by_key(|task| task.id());

        let mut recently_completed = tasks
            .iter()
            .copied()
            .filter(|task| task.status() == TaskStatus::Completed)
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
            .map(TaskProgressItem::from)
            .collect();

        let in_progress = tasks
            .iter()
            .copied()
            .filter(|task| task.status() == TaskStatus::InProgress)
            .map(TaskProgressItem::from)
            .collect();

        let (ready, blocked): (Vec<_>, Vec<_>) = tasks
            .iter()
            .copied()
            .filter(|task| task.status() == TaskStatus::Pending)
            .partition(|task| self.blocking_ids(task.id()).is_ok_and(|ids| ids.is_empty()));
        let ready_omitted = ready.len().saturating_sub(READY_LIMIT);
        let ready = ready
            .into_iter()
            .take(READY_LIMIT)
            .map(TaskProgressItem::from)
            .collect();

        Some(TaskProgressSnapshot {
            task_list_id: batch_id,
            summary: batch.summary().map(str::to_owned),
            task_list_status: batch.status(),
            updated: TaskProgressItem::from(updated),
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
    pub fn lifecycle_snapshot(&self, stale_after_silence_turns: u64) -> TaskLifecycleSnapshot {
        let tasks = self.list();
        let batches = self.list_batches();
        let current_batch = self.current_batch();
        TaskLifecycleSnapshot {
            current_batch,
            stale_after_silence_turns,
            all_completed: detect_batch_all_completed(current_batch, &tasks),
            interrupted: current_batch
                .and_then(|id| detect_interrupted_batch(id, &tasks, &batches, true)),
            stale_batches: detect_stale_batches(&tasks, &batches, stale_after_silence_turns),
        }
    }
}

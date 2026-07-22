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
pub struct TaskReminderItem {
    pub id: TaskId,
    pub subject: String,
    pub status: TaskStatus,
    pub blocked: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskReminderSnapshot {
    pub current_batch: Option<BatchId>,
    pub items: Vec<TaskReminderItem>,
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

    /// Produces Context input for the current Batch without rendering policy.
    pub fn reminder_snapshot(&self) -> TaskReminderSnapshot {
        let current_batch = self.current_batch();
        let items = self
            .list()
            .into_iter()
            .filter(|task| {
                Some(task.batch()) == current_batch && task.status() != TaskStatus::Deleted
            })
            .map(|task| TaskReminderItem {
                id: task.id(),
                subject: task.subject().to_owned(),
                status: task.status(),
                blocked: self.is_blocked(task.id()).unwrap_or(false),
            })
            .collect();
        TaskReminderSnapshot {
            current_batch,
            items,
        }
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

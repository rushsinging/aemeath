use std::collections::{HashMap, HashSet};

use thiserror::Error;

use super::{Batch, BatchId, BatchStatus, Task, TaskId, TaskRevision, TaskStatus, TaskStoreState};

/// A Task-owned, typed persistence snapshot. Runtime entities deliberately do
/// not implement serde; conversion is confined to the wire DTOs in this file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSnapshot {
    revision: TaskRevision,
    tasks: Vec<Task>,
    next_task_id: TaskId,
    next_batch_id: BatchId,
    current_batch: Option<BatchId>,
    batches: Vec<Batch>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TaskSnapshotValidationError {
    #[error("zero task ID: {id}")]
    ZeroTaskId { id: TaskId },
    #[error("zero batch ID: {id}")]
    ZeroBatchId { id: BatchId },
    #[error("duplicate task ID: {id}")]
    DuplicateTaskId { id: TaskId },
    #[error("duplicate batch ID: {id}")]
    DuplicateBatchId { id: BatchId },
    #[error("persisted deleted task: {id}")]
    PersistedDeletedTask { id: TaskId },
    #[error("task {task_id} references missing batch {batch_id}")]
    InvalidBatchReference { task_id: TaskId, batch_id: BatchId },
    #[error("task {task_id} references missing dependency {dependency_id}")]
    DanglingDependency {
        task_id: TaskId,
        dependency_id: TaskId,
    },
    #[error("task {task_id} depends on itself")]
    SelfDependency { task_id: TaskId },
    #[error("duplicate dependency {dependency_id} on task {task_id}")]
    DuplicateDependencyReference {
        task_id: TaskId,
        dependency_id: TaskId,
    },
    #[error("dependency graph contains a cycle")]
    DependencyCycle,
    #[error("cross-batch dependency: {task_id} -> {blocked_by_id}")]
    CrossBatchDependency {
        task_id: TaskId,
        blocked_by_id: TaskId,
    },
    #[error("multiple active batches: {first}, {second}")]
    MultipleActiveBatches { first: BatchId, second: BatchId },
    #[error("invalid current batch: {batch_id}")]
    InvalidCurrentBatch { batch_id: BatchId },
    #[error("current batch {current:?} does not match active batch {active}")]
    CurrentBatchMismatch {
        current: Option<BatchId>,
        active: BatchId,
    },
    #[error("next task ID must exceed every persisted task ID")]
    InvalidNextTaskId,
    #[error("next batch ID must exceed every persisted batch ID")]
    InvalidNextBatchId,
    #[error("invalid timestamps for task {task_id}")]
    InvalidTaskTimestamps { task_id: TaskId },
}

/// A validated restore candidate produced by the Task BC persistence port.
///
/// The type name is public so consumers can name the token that
/// [`crate::TaskPersist::prepare_restore`] returns and
/// [`crate::TaskPersist::commit_restore`] consumes, but it is deliberately
/// opaque: its single field is private, it exposes no accessors, and it
/// implements neither `Clone` nor serde. That keeps the wrapped aggregate state
/// inside the Task BC and makes a prepared token single-use — moving it into
/// `commit_restore` is the only way to install it.
pub struct PreparedTaskRestore {
    candidate: TaskStoreState,
}

impl std::fmt::Debug for PreparedTaskRestore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let _candidate = &self.candidate;
        formatter.write_str("PreparedTaskRestore { .. }")
    }
}

impl PreparedTaskRestore {
    pub(crate) fn into_candidate(self) -> TaskStoreState {
        self.candidate
    }

    #[cfg(test)]
    pub(crate) fn candidate(&self) -> &TaskStoreState {
        &self.candidate
    }
}

impl TaskSnapshot {
    pub(crate) fn from_state(state: &TaskStoreState) -> Self {
        let mut tasks: Vec<_> = state
            .tasks()
            .values()
            .filter(|task| task.status() != TaskStatus::Deleted)
            .cloned()
            .map(|mut task| {
                task.restore_blocks(Vec::new());
                task
            })
            .collect();
        tasks.sort_unstable_by_key(Task::id);
        let mut batches: Vec<_> = state.batches().values().cloned().collect();
        batches.sort_unstable_by_key(Batch::id);
        Self {
            revision: state.revision(),
            tasks,
            next_task_id: state.next_task_id_for_snapshot(),
            next_batch_id: state.next_batch_id_for_snapshot(),
            current_batch: state.current_batch(),
            batches,
        }
    }

    pub fn empty() -> Self {
        Self {
            revision: TaskRevision::new(0),
            tasks: Vec::new(),
            next_task_id: TaskId::new(1),
            next_batch_id: BatchId::new(1),
            current_batch: None,
            batches: Vec::new(),
        }
    }

    pub(crate) fn from_decoded_parts(
        revision: TaskRevision,
        mut tasks: Vec<Task>,
        next_task_id: TaskId,
        next_batch_id: BatchId,
        current_batch: Option<BatchId>,
        mut batches: Vec<Batch>,
    ) -> Self {
        tasks.sort_unstable_by_key(Task::id);
        batches.sort_unstable_by_key(Batch::id);
        Self {
            revision,
            tasks,
            next_task_id,
            next_batch_id,
            current_batch,
            batches,
        }
    }

    pub fn revision(&self) -> TaskRevision {
        self.revision
    }
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }
    pub fn next_task_id(&self) -> TaskId {
        self.next_task_id
    }
    pub fn next_batch_id(&self) -> BatchId {
        self.next_batch_id
    }
    pub fn current_batch(&self) -> Option<BatchId> {
        self.current_batch
    }
    pub fn batches(&self) -> &[Batch] {
        &self.batches
    }

    /// Validates all aggregate invariants without exposing an installation
    /// capability outside the crate.
    pub fn validate(self) -> Result<(), TaskSnapshotValidationError> {
        self.prepare().map(drop)
    }

    /// Validates aggregate invariants and, only after every check succeeds,
    /// builds a crate-private candidate store state. The reverse `blocks` index
    /// is derived from persisted `blocked_by` edges rather than persisted
    /// separately.
    pub(crate) fn prepare(self) -> Result<PreparedTaskRestore, TaskSnapshotValidationError> {
        let mut task_indexes = HashMap::with_capacity(self.tasks.len());
        for (index, task) in self.tasks.iter().enumerate() {
            let id = task.id();
            if id.get() == 0 {
                return Err(TaskSnapshotValidationError::ZeroTaskId { id });
            }
            if task_indexes.insert(id, index).is_some() {
                return Err(TaskSnapshotValidationError::DuplicateTaskId { id });
            }
        }

        let mut batch_indexes = HashMap::with_capacity(self.batches.len());
        let mut active_batch = None;
        for (index, batch) in self.batches.iter().enumerate() {
            let id = batch.id();
            if id.get() == 0 {
                return Err(TaskSnapshotValidationError::ZeroBatchId { id });
            }
            if batch_indexes.insert(id, index).is_some() {
                return Err(TaskSnapshotValidationError::DuplicateBatchId { id });
            }
            if batch.status() == BatchStatus::Active {
                if let Some(first) = active_batch {
                    return Err(TaskSnapshotValidationError::MultipleActiveBatches {
                        first,
                        second: id,
                    });
                }
                active_batch = Some(id);
            }
        }

        for task in &self.tasks {
            let id = task.id();
            if task.status() == TaskStatus::Deleted {
                return Err(TaskSnapshotValidationError::PersistedDeletedTask { id });
            }
            if !batch_indexes.contains_key(&task.batch()) {
                return Err(TaskSnapshotValidationError::InvalidBatchReference {
                    task_id: id,
                    batch_id: task.batch(),
                });
            }
            if !valid_task_timestamps(task) {
                return Err(TaskSnapshotValidationError::InvalidTaskTimestamps { task_id: id });
            }

            let mut dependencies = HashSet::with_capacity(task.blocked_by().len());
            for &dependency_id in task.blocked_by() {
                if dependency_id == id {
                    return Err(TaskSnapshotValidationError::SelfDependency { task_id: id });
                }
                if !dependencies.insert(dependency_id) {
                    return Err(TaskSnapshotValidationError::DuplicateDependencyReference {
                        task_id: id,
                        dependency_id,
                    });
                }
                let Some(&dependency_index) = task_indexes.get(&dependency_id) else {
                    return Err(TaskSnapshotValidationError::DanglingDependency {
                        task_id: id,
                        dependency_id,
                    });
                };
                if self.tasks[dependency_index].batch() != task.batch() {
                    return Err(TaskSnapshotValidationError::CrossBatchDependency {
                        task_id: id,
                        blocked_by_id: dependency_id,
                    });
                }
            }
        }

        if dependency_graph_has_cycle(&self.tasks, &task_indexes) {
            return Err(TaskSnapshotValidationError::DependencyCycle);
        }

        if let Some(current) = self.current_batch {
            let Some(&index) = batch_indexes.get(&current) else {
                return Err(TaskSnapshotValidationError::InvalidCurrentBatch { batch_id: current });
            };
            if self.batches[index].status() != BatchStatus::Active {
                return Err(TaskSnapshotValidationError::InvalidCurrentBatch { batch_id: current });
            }
        }
        if let Some(active) = active_batch {
            if self.current_batch != Some(active) {
                return Err(TaskSnapshotValidationError::CurrentBatchMismatch {
                    current: self.current_batch,
                    active,
                });
            }
        }

        if self.next_task_id.get() == 0
            || self.tasks.iter().any(|task| task.id() >= self.next_task_id)
        {
            return Err(TaskSnapshotValidationError::InvalidNextTaskId);
        }
        if self.next_batch_id.get() == 0
            || self
                .batches
                .iter()
                .any(|batch| batch.id() >= self.next_batch_id)
        {
            return Err(TaskSnapshotValidationError::InvalidNextBatchId);
        }

        // All validation is complete. Build the candidate and its derived
        // reverse dependency index without altering persisted timestamps.
        let mut tasks: HashMap<_, _> = self
            .tasks
            .into_iter()
            .map(|task| (task.id(), task))
            .collect();
        let mut reverse: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
        for task in tasks.values() {
            for &dependency in task.blocked_by() {
                reverse.entry(dependency).or_default().push(task.id());
            }
        }
        for (id, blocks) in reverse {
            tasks
                .get_mut(&id)
                .expect("validated dependency must exist")
                .restore_blocks(blocks);
        }
        let batches = self
            .batches
            .into_iter()
            .map(|batch| (batch.id(), batch))
            .collect();
        Ok(PreparedTaskRestore {
            candidate: TaskStoreState::from_snapshot(
                tasks,
                batches,
                self.next_task_id,
                self.next_batch_id,
                self.current_batch,
                self.revision,
            ),
        })
    }
}

fn valid_task_timestamps(task: &Task) -> bool {
    let created = task.created_at();
    let updated = task.updated_at();
    if updated < created
        || task
            .started_at()
            .is_some_and(|started| started < created || started > updated)
        || task
            .completed_at()
            .is_some_and(|completed| completed < created || completed > updated)
        || matches!((task.started_at(), task.completed_at()), (Some(started), Some(completed)) if completed < started)
    {
        return false;
    }
    match task.status() {
        TaskStatus::Pending => task.started_at().is_none() && task.completed_at().is_none(),
        TaskStatus::InProgress => task.started_at().is_some() && task.completed_at().is_none(),
        TaskStatus::Completed => task.started_at().is_some() && task.completed_at().is_some(),
        TaskStatus::Deleted => false,
    }
}

fn dependency_graph_has_cycle(tasks: &[Task], indexes: &HashMap<TaskId, usize>) -> bool {
    // Kahn's algorithm avoids making validation depth depend on the native
    // stack. Build adjacency by task-slice index so traversal is deterministic
    // and never depends on HashMap iteration order.
    let mut remaining_dependencies = vec![0usize; tasks.len()];
    let mut dependents = vec![Vec::new(); tasks.len()];
    for (task_index, task) in tasks.iter().enumerate() {
        remaining_dependencies[task_index] = task.blocked_by().len();
        for dependency_id in task.blocked_by() {
            dependents[indexes[dependency_id]].push(task_index);
        }
    }

    let mut ready: Vec<usize> = remaining_dependencies
        .iter()
        .enumerate()
        .filter_map(|(index, &count)| (count == 0).then_some(index))
        .collect();
    let mut visited = 0;
    while let Some(index) = ready.pop() {
        visited += 1;
        for &dependent in &dependents[index] {
            remaining_dependencies[dependent] -= 1;
            if remaining_dependencies[dependent] == 0 {
                ready.push(dependent);
            }
        }
    }

    visited != tasks.len()
}

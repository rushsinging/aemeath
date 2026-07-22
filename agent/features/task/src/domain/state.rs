use std::collections::{HashMap, HashSet};

use super::{
    Batch, BatchCreateSpec, BatchId, BatchStatus, Task, TaskCommandError, TaskCommandResult,
    TaskCreateSpec, TaskEvent, TaskId, TaskPriority, TaskRevision, TaskStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStoreState {
    tasks: HashMap<TaskId, Task>,
    batches: HashMap<BatchId, Batch>,
    next_task_id: TaskId,
    next_batch_id: BatchId,
    current_batch: Option<BatchId>,
    revision: TaskRevision,
}

impl TaskStoreState {
    pub(crate) fn from_snapshot(
        tasks: HashMap<TaskId, Task>,
        batches: HashMap<BatchId, Batch>,
        next_task_id: TaskId,
        next_batch_id: BatchId,
        current_batch: Option<BatchId>,
        revision: TaskRevision,
    ) -> Self {
        Self {
            tasks,
            batches,
            next_task_id,
            next_batch_id,
            current_batch,
            revision,
        }
    }

    /// Captures all persisted aggregate fields from this state. Tombstones are
    /// excluded, and the reverse `blocks` index remains runtime-derived data.
    pub(crate) fn capture_snapshot(&self) -> super::TaskSnapshot {
        super::TaskSnapshot::from_state(self)
    }

    pub fn empty() -> Self {
        Self {
            tasks: HashMap::new(),
            batches: HashMap::new(),
            next_task_id: TaskId::new(1),
            next_batch_id: BatchId::new(1),
            current_batch: None,
            revision: TaskRevision::new(0),
        }
    }
    pub(crate) fn tasks(&self) -> &HashMap<TaskId, Task> {
        &self.tasks
    }
    pub(crate) fn batches(&self) -> &HashMap<BatchId, Batch> {
        &self.batches
    }
    pub(crate) fn next_task_id_for_snapshot(&self) -> TaskId {
        self.next_task_id
    }
    pub(crate) fn next_batch_id_for_snapshot(&self) -> BatchId {
        self.next_batch_id
    }
    #[cfg(test)]
    pub(crate) fn next_task_id(&self) -> TaskId {
        self.next_task_id
    }
    #[cfg(test)]
    pub(crate) fn next_batch_id(&self) -> BatchId {
        self.next_batch_id
    }
    pub fn current_batch(&self) -> Option<BatchId> {
        self.current_batch
    }
    /// Authoritative monotonic revision of the last successful, state-changing
    /// mutation; empty store starts at `0`. Failed commands and idempotent
    /// no-ops never advance it.
    pub fn revision(&self) -> TaskRevision {
        self.revision
    }

    #[cfg(test)]
    pub(crate) fn with_next_task_id(mut self, id: TaskId) -> Self {
        self.next_task_id = id;
        self
    }
    #[cfg(test)]
    pub(crate) fn with_next_batch_id(mut self, id: BatchId) -> Self {
        self.next_batch_id = id;
        self
    }
    #[cfg(test)]
    pub(crate) fn with_revision(mut self, revision: TaskRevision) -> Self {
        self.revision = revision;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_batch(mut self, batch: Batch) -> Self {
        if batch.status() == BatchStatus::Active {
            self.current_batch = Some(batch.id());
        }
        self.batches.insert(batch.id(), batch);
        self
    }

    /// Reserves the next revision without mutating any state; callers MUST
    /// perform this before touching maps/counters so a `RevisionExhausted`
    /// error leaves the whole command a true no-op.
    fn reserve_revision(&self) -> Result<TaskRevision, TaskCommandError> {
        self.revision
            .get()
            .checked_add(1)
            .map(TaskRevision::new)
            .ok_or(TaskCommandError::RevisionExhausted)
    }

    /// Commits an already-reserved revision atomically with the mutation
    /// result that produced it.
    fn commit<T>(
        &mut self,
        mut result: TaskCommandResult<T>,
        revision: TaskRevision,
    ) -> TaskCommandResult<T> {
        self.revision = revision;
        result.commit(revision);
        result
    }

    /// Atomically clears all Tasks and Batches and resets allocation counters.
    ///
    /// A non-empty aggregate is one state-changing command: it emits one
    /// `TaskStoreCleared` event and advances revision exactly once. An already
    /// empty aggregate is an idempotent no-op. The monotonic revision is never
    /// reset to zero.
    pub fn clear(&mut self) -> Result<TaskCommandResult<()>, TaskCommandError> {
        if self.tasks.is_empty()
            && self.batches.is_empty()
            && self.current_batch.is_none()
            && self.next_batch_id == BatchId::new(1)
        {
            return Ok(TaskCommandResult::uncommitted((), Vec::new()));
        }

        let revision = self.reserve_revision()?;
        let events = vec![TaskEvent::TaskStoreCleared {
            task_count: self.tasks.len(),
            batch_count: self.batches.len(),
        }];
        self.tasks.clear();
        self.batches.clear();
        self.next_batch_id = BatchId::new(1);
        self.current_batch = None;
        Ok(self.commit(TaskCommandResult::uncommitted((), events), revision))
    }

    pub fn create_batch(
        &mut self,
        spec: BatchCreateSpec,
        timestamp: u64,
    ) -> Result<TaskCommandResult<Batch>, TaskCommandError> {
        let id = self.next_batch_id;
        let next_batch_id = id
            .get()
            .checked_add(1)
            .map(BatchId::new)
            .ok_or(TaskCommandError::BatchIdExhausted)?;
        let revision = self.reserve_revision()?;
        if let Some(active) = self.current_batch {
            self.batches
                .get_mut(&active)
                .expect("current batch must exist")
                .transition_to(BatchStatus::Archived)
                .expect("current batch must be active");
        }
        let batch = Batch::create(id, spec, timestamp);
        self.batches.insert(id, batch.clone());
        self.current_batch = Some(id);
        self.next_batch_id = next_batch_id;
        Ok(self.commit(TaskCommandResult::uncommitted(batch, Vec::new()), revision))
    }

    pub fn create_task(
        &mut self,
        spec: TaskCreateSpec,
        timestamp: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError> {
        let batch = self.current_batch.ok_or(TaskCommandError::NoActiveBatch)?;
        let id = self.next_task_id;
        let next_task_id = id
            .get()
            .checked_add(1)
            .map(TaskId::new)
            .ok_or(TaskCommandError::TaskIdExhausted)?;
        let revision = self.reserve_revision()?;
        let seq = self
            .tasks
            .values()
            .filter(|task| task.batch() == batch)
            .map(Task::seq)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(TaskCommandError::TaskIdExhausted)?;
        let result = Task::create(id, batch, seq, spec, timestamp);
        self.tasks.insert(id, result.value.clone());
        self.next_task_id = next_task_id;
        Ok(self.commit(result, revision))
    }

    pub fn set_subject(
        &mut self,
        id: TaskId,
        subject: String,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError> {
        let task = self
            .tasks
            .get(&id)
            .filter(|task| task.status() != TaskStatus::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        if subject.trim().is_empty() {
            return Err(TaskCommandError::InvalidTaskSubject);
        }
        if task.subject() == subject {
            return Ok(TaskCommandResult::uncommitted(task.clone(), Vec::new()));
        }
        let revision = self.reserve_revision()?;
        let task = self.tasks.get_mut(&id).expect("validated task must exist");
        let result = task.set_subject(subject, updated_at)?;
        Ok(self.commit(result, revision))
    }

    pub fn set_description(
        &mut self,
        id: TaskId,
        description: String,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError> {
        let task = self
            .tasks
            .get(&id)
            .filter(|task| task.status() != TaskStatus::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        if task.description() == description {
            return Ok(TaskCommandResult::uncommitted(task.clone(), Vec::new()));
        }
        let revision = self.reserve_revision()?;
        let task = self.tasks.get_mut(&id).expect("validated task must exist");
        let result = task.set_description(description, updated_at);
        Ok(self.commit(result, revision))
    }

    pub fn set_priority(
        &mut self,
        id: TaskId,
        priority: TaskPriority,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError> {
        let task = self
            .tasks
            .get(&id)
            .filter(|task| task.status() != TaskStatus::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        let from = task.priority();
        if from == priority {
            return Ok(TaskCommandResult::uncommitted(task.clone(), Vec::new()));
        }
        let revision = self.reserve_revision()?;
        let task = self.tasks.get_mut(&id).expect("validated task must exist");
        task.set_priority(priority, updated_at);
        let snapshot = task.clone();
        Ok(self.commit(
            TaskCommandResult::uncommitted(
                snapshot,
                vec![TaskEvent::TaskPriorityChanged {
                    task_id: id,
                    from,
                    to: priority,
                }],
            ),
            revision,
        ))
    }

    pub fn add_tag(
        &mut self,
        id: TaskId,
        tag: String,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError> {
        let task = self
            .tasks
            .get(&id)
            .filter(|task| task.status() != TaskStatus::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        if task.tags().contains(&tag) {
            return Ok(TaskCommandResult::uncommitted(task.clone(), Vec::new()));
        }
        let revision = self.reserve_revision()?;
        let task = self.tasks.get_mut(&id).expect("validated task must exist");
        task.add_tag(tag.clone(), updated_at);
        let snapshot = task.clone();
        Ok(self.commit(
            TaskCommandResult::uncommitted(
                snapshot,
                vec![TaskEvent::TaskTagAdded { task_id: id, tag }],
            ),
            revision,
        ))
    }

    pub fn remove_tag(
        &mut self,
        id: TaskId,
        tag: &str,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError> {
        let task = self
            .tasks
            .get(&id)
            .filter(|task| task.status() != TaskStatus::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        if !task.tags().iter().any(|existing| existing == tag) {
            return Ok(TaskCommandResult::uncommitted(task.clone(), Vec::new()));
        }
        let revision = self.reserve_revision()?;
        let task = self.tasks.get_mut(&id).expect("validated task must exist");
        task.remove_tag(tag, updated_at);
        let snapshot = task.clone();
        Ok(self.commit(
            TaskCommandResult::uncommitted(
                snapshot,
                vec![TaskEvent::TaskTagRemoved {
                    task_id: id,
                    tag: tag.to_string(),
                }],
            ),
            revision,
        ))
    }

    pub fn add_dependency(
        &mut self,
        task_id: TaskId,
        blocked_by_id: TaskId,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError> {
        let task = self
            .tasks
            .get(&task_id)
            .filter(|task| task.status() != TaskStatus::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id: task_id })?;
        let blocker = self
            .tasks
            .get(&blocked_by_id)
            .filter(|task| task.status() != TaskStatus::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id: blocked_by_id })?;
        if task.batch() != blocker.batch() {
            return Err(TaskCommandError::CrossBatchDependency {
                task_id,
                blocked_by_id,
            });
        }
        if task.blocked_by().contains(&blocked_by_id) {
            return Ok(TaskCommandResult::uncommitted(task.clone(), Vec::new()));
        }
        if self.would_create_cycle(task_id, blocked_by_id) {
            return Err(TaskCommandError::DependencyCycle {
                task_id,
                blocked_by_id,
            });
        }
        let revision = self.reserve_revision()?;
        self.tasks
            .get_mut(&task_id)
            .expect("validated task must exist")
            .add_blocked_by(blocked_by_id, updated_at);
        self.tasks
            .get_mut(&blocked_by_id)
            .expect("validated blocker must exist")
            .add_blocks(task_id, updated_at);
        let snapshot = self
            .tasks
            .get(&task_id)
            .expect("validated task must exist")
            .clone();
        Ok(self.commit(
            TaskCommandResult::uncommitted(
                snapshot,
                vec![TaskEvent::TaskDependencyAdded {
                    task_id,
                    blocked_by_id,
                }],
            ),
            revision,
        ))
    }

    pub fn remove_dependency(
        &mut self,
        task_id: TaskId,
        blocked_by_id: TaskId,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError> {
        let task = self
            .tasks
            .get(&task_id)
            .ok_or(TaskCommandError::TaskNotFound { id: task_id })?;
        if !self.tasks.contains_key(&blocked_by_id) {
            return Err(TaskCommandError::TaskNotFound { id: blocked_by_id });
        }
        if !task.blocked_by().contains(&blocked_by_id) {
            return Ok(TaskCommandResult::uncommitted(task.clone(), Vec::new()));
        }
        let revision = self.reserve_revision()?;
        self.tasks
            .get_mut(&task_id)
            .expect("validated task must exist")
            .remove_blocked_by(blocked_by_id, updated_at);
        self.tasks
            .get_mut(&blocked_by_id)
            .expect("validated blocker must exist")
            .remove_blocks(task_id, updated_at);
        let snapshot = self
            .tasks
            .get(&task_id)
            .expect("validated task must exist")
            .clone();
        Ok(self.commit(
            TaskCommandResult::uncommitted(
                snapshot,
                vec![TaskEvent::TaskDependencyRemoved {
                    task_id,
                    blocked_by_id,
                }],
            ),
            revision,
        ))
    }

    pub fn would_create_cycle(&self, task_id: TaskId, blocked_by_id: TaskId) -> bool {
        if task_id == blocked_by_id {
            return true;
        }
        let mut visited = HashSet::new();
        let mut stack = vec![blocked_by_id];
        while let Some(current) = stack.pop() {
            if current == task_id {
                return true;
            }
            if !visited.insert(current) {
                continue;
            }
            if let Some(task) = self.tasks.get(&current) {
                stack.extend(task.blocked_by().iter().copied());
            }
        }
        false
    }

    pub fn pause_batch(
        &mut self,
        id: BatchId,
    ) -> Result<TaskCommandResult<Batch>, TaskCommandError> {
        let batch = self
            .batches
            .get(&id)
            .ok_or(TaskCommandError::BatchNotFound { id })?;
        let mut dry_run = batch.clone();
        dry_run.transition_to(BatchStatus::Paused)?;
        let revision = self.reserve_revision()?;
        let batch = self
            .batches
            .get_mut(&id)
            .expect("validated batch must exist");
        batch
            .transition_to(BatchStatus::Paused)
            .expect("legality pre-validated above");
        if self.current_batch == Some(id) {
            self.current_batch = None;
        }
        let snapshot = self
            .batches
            .get(&id)
            .expect("validated batch must exist")
            .clone();
        Ok(self.commit(
            TaskCommandResult::uncommitted(snapshot, Vec::new()),
            revision,
        ))
    }

    pub fn resume_batch(
        &mut self,
        id: BatchId,
    ) -> Result<TaskCommandResult<Batch>, TaskCommandError> {
        let batch = self
            .batches
            .get(&id)
            .ok_or(TaskCommandError::BatchNotFound { id })?;
        if let Some(active) = self.current_batch {
            if active != id {
                return Err(TaskCommandError::ActiveBatchConflict {
                    active,
                    requested: id,
                });
            }
        }
        let mut dry_run = batch.clone();
        dry_run.transition_to(BatchStatus::Active)?;
        let revision = self.reserve_revision()?;
        let batch = self
            .batches
            .get_mut(&id)
            .expect("validated batch must exist");
        batch
            .transition_to(BatchStatus::Active)
            .expect("legality pre-validated above");
        self.current_batch = Some(id);
        let snapshot = self
            .batches
            .get(&id)
            .expect("validated batch must exist")
            .clone();
        Ok(self.commit(
            TaskCommandResult::uncommitted(snapshot, Vec::new()),
            revision,
        ))
    }

    pub fn archive_batch(
        &mut self,
        id: BatchId,
    ) -> Result<TaskCommandResult<Batch>, TaskCommandError> {
        let batch = self
            .batches
            .get(&id)
            .ok_or(TaskCommandError::BatchNotFound { id })?;
        // Archiving is a terminal, idempotent transition: repeat calls on an
        // already-archived batch are a true no-op and must never reserve a
        // revision, so they keep succeeding even once the revision counter
        // is exhausted.
        if batch.status() == BatchStatus::Archived {
            return Ok(TaskCommandResult::uncommitted(batch.clone(), Vec::new()));
        }
        let mut dry_run = batch.clone();
        dry_run.transition_to(BatchStatus::Archived)?;
        let revision = self.reserve_revision()?;
        let batch = self
            .batches
            .get_mut(&id)
            .expect("validated batch must exist");
        batch
            .transition_to(BatchStatus::Archived)
            .expect("legality pre-validated above");
        if self.current_batch == Some(id) {
            self.current_batch = None;
        }
        let snapshot = self
            .batches
            .get(&id)
            .expect("validated batch must exist")
            .clone();
        Ok(self.commit(
            TaskCommandResult::uncommitted(snapshot, Vec::new()),
            revision,
        ))
    }

    /// Runtime calls this once per Batch at the end of every turn to atomically
    /// update `last_active_turn` / `silence_turns`; `active` reports whether the
    /// turn produced any activity for this Batch. Only an `Active` batch may be
    /// updated; `Paused`/`Archived` batches return a typed error and are left
    /// completely unchanged. Calls that would not change any observable field
    /// are idempotent no-ops and never advance the revision.
    pub fn record_batch_turn(
        &mut self,
        id: BatchId,
        turn: u64,
        active: bool,
    ) -> Result<TaskCommandResult<Batch>, TaskCommandError> {
        let batch = self
            .batches
            .get(&id)
            .ok_or(TaskCommandError::BatchNotFound { id })?;
        let mut dry_run = batch.clone();
        let changed = dry_run.record_turn(turn, active)?;
        if !changed {
            return Ok(TaskCommandResult::uncommitted(dry_run, Vec::new()));
        }
        let revision = self.reserve_revision()?;
        let batch = self
            .batches
            .get_mut(&id)
            .expect("validated batch must exist");
        batch
            .record_turn(turn, active)
            .expect("legality and effectiveness pre-validated above");
        let snapshot = batch.clone();
        Ok(self.commit(
            TaskCommandResult::uncommitted(snapshot, Vec::new()),
            revision,
        ))
    }

    pub fn is_blocked(&self, id: TaskId) -> Result<bool, TaskCommandError> {
        Ok(!self.blocking_ids(id)?.is_empty())
    }

    fn blocking_ids(&self, id: TaskId) -> Result<Vec<TaskId>, TaskCommandError> {
        let task = self
            .tasks
            .get(&id)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        Ok(task
            .blocked_by()
            .iter()
            .copied()
            .filter(|dependency_id| {
                self.tasks.get(dependency_id).is_some_and(|dependency| {
                    !matches!(
                        dependency.status(),
                        TaskStatus::Completed | TaskStatus::Deleted
                    )
                })
            })
            .collect())
    }

    pub fn transition(
        &mut self,
        id: TaskId,
        to: TaskStatus,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError> {
        if to == TaskStatus::InProgress {
            let blocked_by = self.blocking_ids(id)?;
            if !blocked_by.is_empty() {
                return Err(TaskCommandError::TaskBlocked { id, blocked_by });
            }
        }
        let current = self
            .tasks
            .get(&id)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        let mut dry_run = current.clone();
        dry_run.transition_to(to, updated_at)?;
        let revision = self.reserve_revision()?;
        let result = self
            .tasks
            .get_mut(&id)
            .expect("validated task must exist")
            .transition_to(to, updated_at)
            .expect("legality pre-validated above");
        Ok(self.commit(result, revision))
    }

    /// Removes all incoming/outgoing dependency edges and marks the Task
    /// `Deleted` in one commit. Repeated delete of an already-`Deleted` Task
    /// is an idempotent no-op: it returns the current snapshot with empty
    /// `events` and `revision() == None`, and never reserves a new revision.
    pub fn delete(
        &mut self,
        id: TaskId,
        updated_at: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError> {
        let (blocked_by, blocks) = {
            let task = self
                .tasks
                .get(&id)
                .ok_or(TaskCommandError::TaskNotFound { id })?;
            if task.status() == TaskStatus::Deleted {
                return Ok(TaskCommandResult::uncommitted(task.clone(), Vec::new()));
            }
            (task.blocked_by().to_vec(), task.blocks().to_vec())
        };
        let revision = self.reserve_revision()?;
        for dependency_id in &blocked_by {
            self.tasks
                .get_mut(dependency_id)
                .expect("dependency graph endpoint must exist")
                .remove_blocks(id, updated_at);
        }
        for dependent_id in &blocks {
            self.tasks
                .get_mut(dependent_id)
                .expect("dependency graph endpoint must exist")
                .remove_blocked_by(id, updated_at);
        }
        let task = self.tasks.get_mut(&id).expect("validated task must exist");
        for dependency_id in blocked_by {
            task.remove_blocked_by(dependency_id, updated_at);
        }
        for dependent_id in blocks {
            task.remove_blocks(dependent_id, updated_at);
        }
        task.mark_deleted(updated_at);
        let snapshot = task.clone();
        Ok(self.commit(
            TaskCommandResult::uncommitted(snapshot, vec![TaskEvent::TaskDeleted { task_id: id }]),
            revision,
        ))
    }
}

impl Default for TaskStoreState {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;

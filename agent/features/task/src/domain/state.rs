use std::collections::{HashMap, HashSet};

use super::{
    BatchCreateSpecData, BatchData, BatchIdData, BatchStatusData, TaskCommandError,
    TaskCommandResultData, TaskCreateSpecData, TaskData, TaskEventData, TaskIdData,
    TaskPriorityData, TaskRevisionData, TaskStatusData,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStoreState {
    tasks: HashMap<TaskIdData, TaskData>,
    batches: HashMap<BatchIdData, BatchData>,
    next_task_id: TaskIdData,
    next_batch_id: BatchIdData,
    current_batch: Option<BatchIdData>,
    revision: TaskRevisionData,
}

impl TaskStoreState {
    pub(crate) fn from_snapshot(
        tasks: HashMap<TaskIdData, TaskData>,
        batches: HashMap<BatchIdData, BatchData>,
        next_task_id: TaskIdData,
        next_batch_id: BatchIdData,
        current_batch: Option<BatchIdData>,
        revision: TaskRevisionData,
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
    pub(crate) fn capture_snapshot(&self) -> super::TaskSnapshotData {
        super::TaskSnapshotData::from_state(self)
    }

    pub fn empty() -> Self {
        Self {
            tasks: HashMap::new(),
            batches: HashMap::new(),
            next_task_id: TaskIdData::new(1),
            next_batch_id: BatchIdData::new(1),
            current_batch: None,
            revision: TaskRevisionData::new(0),
        }
    }
    pub(crate) fn tasks(&self) -> &HashMap<TaskIdData, TaskData> {
        &self.tasks
    }
    pub(crate) fn batches(&self) -> &HashMap<BatchIdData, BatchData> {
        &self.batches
    }
    pub(crate) fn next_task_id_for_snapshot(&self) -> TaskIdData {
        self.next_task_id
    }
    pub(crate) fn next_batch_id_for_snapshot(&self) -> BatchIdData {
        self.next_batch_id
    }
    #[cfg(test)]
    pub(crate) fn next_task_id(&self) -> TaskIdData {
        self.next_task_id
    }
    #[cfg(test)]
    pub(crate) fn next_batch_id(&self) -> BatchIdData {
        self.next_batch_id
    }
    pub fn current_batch(&self) -> Option<BatchIdData> {
        self.current_batch
    }
    /// Authoritative monotonic revision of the last successful, state-changing
    /// mutation; empty store starts at `0`. Failed commands and idempotent
    /// no-ops never advance it.
    pub fn revision(&self) -> TaskRevisionData {
        self.revision
    }

    #[cfg(test)]
    pub(crate) fn with_next_task_id(mut self, id: TaskIdData) -> Self {
        self.next_task_id = id;
        self
    }
    #[cfg(test)]
    pub(crate) fn with_next_batch_id(mut self, id: BatchIdData) -> Self {
        self.next_batch_id = id;
        self
    }
    #[cfg(test)]
    pub(crate) fn with_revision(mut self, revision: TaskRevisionData) -> Self {
        self.revision = revision;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_batch(mut self, batch: BatchData) -> Self {
        if batch.status() == BatchStatusData::Active {
            self.current_batch = Some(batch.id());
        }
        self.batches.insert(batch.id(), batch);
        self
    }

    /// Reserves the next revision without mutating any state; callers MUST
    /// perform this before touching maps/counters so a `RevisionExhausted`
    /// error leaves the whole command a true no-op.
    fn reserve_revision(&self) -> Result<TaskRevisionData, share::error::DomainError> {
        self.revision
            .get()
            .checked_add(1)
            .map(TaskRevisionData::new)
            .ok_or_else(|| share::error::DomainError::from(TaskCommandError::RevisionExhausted))
    }

    /// Commits an already-reserved revision atomically with the mutation
    /// result that produced it.
    fn commit<T>(
        &mut self,
        mut result: TaskCommandResultData<T>,
        revision: TaskRevisionData,
    ) -> TaskCommandResultData<T> {
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
    pub fn clear(&mut self) -> Result<TaskCommandResultData<()>, share::error::DomainError> {
        if self.tasks.is_empty()
            && self.batches.is_empty()
            && self.current_batch.is_none()
            && self.next_batch_id == BatchIdData::new(1)
        {
            return Ok(TaskCommandResultData::uncommitted((), Vec::new()));
        }

        let revision = self.reserve_revision()?;
        let events = vec![TaskEventData::TaskStoreCleared {
            task_count: self.tasks.len(),
            batch_count: self.batches.len(),
        }];
        self.tasks.clear();
        self.batches.clear();
        self.next_batch_id = BatchIdData::new(1);
        self.current_batch = None;
        Ok(self.commit(TaskCommandResultData::uncommitted((), events), revision))
    }

    pub fn create_batch(
        &mut self,
        spec: BatchCreateSpecData,
        timestamp: u64,
    ) -> Result<TaskCommandResultData<BatchData>, share::error::DomainError> {
        let id = self.next_batch_id;
        let next_batch_id = id
            .get()
            .checked_add(1)
            .map(BatchIdData::new)
            .ok_or_else(|| share::error::DomainError::from(TaskCommandError::BatchIdExhausted))?;
        let revision = self.reserve_revision()?;
        if let Some(active) = self.current_batch {
            self.batches
                .get_mut(&active)
                .expect("current batch must exist")
                .transition_to(BatchStatusData::Archived)
                .expect("current batch must be active");
        }
        let batch = BatchData::create(id, spec, timestamp);
        self.batches.insert(id, batch.clone());
        self.current_batch = Some(id);
        self.next_batch_id = next_batch_id;
        Ok(self.commit(
            TaskCommandResultData::uncommitted(batch, Vec::new()),
            revision,
        ))
    }

    pub fn create_task(
        &mut self,
        spec: TaskCreateSpecData,
        timestamp: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        let batch = self
            .current_batch
            .ok_or_else(|| share::error::DomainError::from(TaskCommandError::NoActiveBatch))?;
        let id = self.next_task_id;
        let next_task_id = id
            .get()
            .checked_add(1)
            .map(TaskIdData::new)
            .ok_or_else(|| share::error::DomainError::from(TaskCommandError::TaskIdExhausted))?;
        let revision = self.reserve_revision()?;
        let seq = self
            .tasks
            .values()
            .filter(|task| task.batch() == batch)
            .map(TaskData::seq)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| share::error::DomainError::from(TaskCommandError::TaskIdExhausted))?;
        let result = TaskData::create(id, batch, seq, spec, timestamp);
        self.tasks.insert(id, result.value.clone());
        self.next_task_id = next_task_id;
        Ok(self.commit(result, revision))
    }

    pub fn set_subject(
        &mut self,
        id: TaskIdData,
        subject: String,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        let task = self
            .tasks
            .get(&id)
            .filter(|task| task.status() != TaskStatusData::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        if subject.trim().is_empty() {
            return Err(TaskCommandError::InvalidTaskSubject.into());
        }
        if task.subject() == subject {
            return Ok(TaskCommandResultData::uncommitted(task.clone(), Vec::new()));
        }
        let revision = self.reserve_revision()?;
        let task = self.tasks.get_mut(&id).expect("validated task must exist");
        let result = task.set_subject(subject, updated_at)?;
        Ok(self.commit(result, revision))
    }

    pub fn set_description(
        &mut self,
        id: TaskIdData,
        description: String,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        let task = self
            .tasks
            .get(&id)
            .filter(|task| task.status() != TaskStatusData::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        if task.description() == description {
            return Ok(TaskCommandResultData::uncommitted(task.clone(), Vec::new()));
        }
        let revision = self.reserve_revision()?;
        let task = self.tasks.get_mut(&id).expect("validated task must exist");
        let result = task.set_description(description, updated_at);
        Ok(self.commit(result, revision))
    }

    pub fn set_priority(
        &mut self,
        id: TaskIdData,
        priority: TaskPriorityData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        let task = self
            .tasks
            .get(&id)
            .filter(|task| task.status() != TaskStatusData::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        let from = task.priority();
        if from == priority {
            return Ok(TaskCommandResultData::uncommitted(task.clone(), Vec::new()));
        }
        let revision = self.reserve_revision()?;
        let task = self.tasks.get_mut(&id).expect("validated task must exist");
        task.set_priority(priority, updated_at);
        let snapshot = task.clone();
        Ok(self.commit(
            TaskCommandResultData::uncommitted(
                snapshot,
                vec![TaskEventData::TaskPriorityChanged {
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
        id: TaskIdData,
        tag: String,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        let task = self
            .tasks
            .get(&id)
            .filter(|task| task.status() != TaskStatusData::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        if task.tags().contains(&tag) {
            return Ok(TaskCommandResultData::uncommitted(task.clone(), Vec::new()));
        }
        let revision = self.reserve_revision()?;
        let task = self.tasks.get_mut(&id).expect("validated task must exist");
        task.add_tag(tag.clone(), updated_at);
        let snapshot = task.clone();
        Ok(self.commit(
            TaskCommandResultData::uncommitted(
                snapshot,
                vec![TaskEventData::TaskTagAdded { task_id: id, tag }],
            ),
            revision,
        ))
    }

    pub fn remove_tag(
        &mut self,
        id: TaskIdData,
        tag: &str,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        let task = self
            .tasks
            .get(&id)
            .filter(|task| task.status() != TaskStatusData::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        if !task.tags().iter().any(|existing| existing == tag) {
            return Ok(TaskCommandResultData::uncommitted(task.clone(), Vec::new()));
        }
        let revision = self.reserve_revision()?;
        let task = self.tasks.get_mut(&id).expect("validated task must exist");
        task.remove_tag(tag, updated_at);
        let snapshot = task.clone();
        Ok(self.commit(
            TaskCommandResultData::uncommitted(
                snapshot,
                vec![TaskEventData::TaskTagRemoved {
                    task_id: id,
                    tag: tag.to_string(),
                }],
            ),
            revision,
        ))
    }

    pub fn add_dependency(
        &mut self,
        task_id: TaskIdData,
        blocked_by_id: TaskIdData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        let task = self
            .tasks
            .get(&task_id)
            .filter(|task| task.status() != TaskStatusData::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id: task_id })?;
        let blocker = self
            .tasks
            .get(&blocked_by_id)
            .filter(|task| task.status() != TaskStatusData::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id: blocked_by_id })?;
        if task.batch() != blocker.batch() {
            return Err(TaskCommandError::CrossBatchDependency {
                task_id,
                blocked_by_id,
            }
            .into());
        }
        if task.blocked_by().contains(&blocked_by_id) {
            return Ok(TaskCommandResultData::uncommitted(task.clone(), Vec::new()));
        }
        if self.would_create_cycle(task_id, blocked_by_id) {
            return Err(TaskCommandError::DependencyCycle {
                task_id,
                blocked_by_id,
            }
            .into());
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
            TaskCommandResultData::uncommitted(
                snapshot,
                vec![TaskEventData::TaskDependencyAdded {
                    task_id,
                    blocked_by_id,
                }],
            ),
            revision,
        ))
    }

    pub fn replace_dependencies(
        &mut self,
        task_id: TaskIdData,
        mut blocked_by_ids: Vec<TaskIdData>,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        let task = self
            .tasks
            .get(&task_id)
            .filter(|task| task.status() != TaskStatusData::Deleted)
            .ok_or(TaskCommandError::TaskNotFound { id: task_id })?;
        let batch = task.batch();
        let current = task.blocked_by().to_vec();

        blocked_by_ids.sort_unstable();
        for pair in blocked_by_ids.windows(2) {
            if pair[0] == pair[1] {
                return Err(TaskCommandError::DuplicateDependency {
                    task_id,
                    blocked_by_id: pair[0],
                }
                .into());
            }
        }
        for blocked_by_id in &blocked_by_ids {
            let blocker = self
                .tasks
                .get(blocked_by_id)
                .filter(|task| task.status() != TaskStatusData::Deleted)
                .ok_or(TaskCommandError::TaskNotFound { id: *blocked_by_id })?;
            if blocker.batch() != batch {
                return Err(TaskCommandError::CrossBatchDependency {
                    task_id,
                    blocked_by_id: *blocked_by_id,
                }
                .into());
            }
        }
        if current == blocked_by_ids {
            return Ok(TaskCommandResultData::uncommitted(task.clone(), Vec::new()));
        }

        let mut dry_run = self.clone();
        dry_run.revision = TaskRevisionData::new(0);
        for blocked_by_id in &current {
            dry_run.remove_dependency(task_id, *blocked_by_id, updated_at)?;
        }
        for blocked_by_id in &blocked_by_ids {
            dry_run.add_dependency(task_id, *blocked_by_id, updated_at)?;
        }

        let revision = self.reserve_revision()?;
        let removed = current
            .iter()
            .copied()
            .filter(|id| !blocked_by_ids.contains(id))
            .collect::<Vec<_>>();
        let added = blocked_by_ids
            .iter()
            .copied()
            .filter(|id| !current.contains(id))
            .collect::<Vec<_>>();
        for blocked_by_id in &removed {
            self.tasks
                .get_mut(&task_id)
                .expect("validated task must exist")
                .remove_blocked_by(*blocked_by_id, updated_at);
            self.tasks
                .get_mut(blocked_by_id)
                .expect("validated blocker must exist")
                .remove_blocks(task_id, updated_at);
        }
        for blocked_by_id in &added {
            self.tasks
                .get_mut(&task_id)
                .expect("validated task must exist")
                .add_blocked_by(*blocked_by_id, updated_at);
            self.tasks
                .get_mut(blocked_by_id)
                .expect("validated blocker must exist")
                .add_blocks(task_id, updated_at);
        }
        let snapshot = self
            .tasks
            .get(&task_id)
            .expect("validated task must exist")
            .clone();
        let events = removed
            .into_iter()
            .map(|blocked_by_id| TaskEventData::TaskDependencyRemoved {
                task_id,
                blocked_by_id,
            })
            .chain(
                added
                    .into_iter()
                    .map(|blocked_by_id| TaskEventData::TaskDependencyAdded {
                        task_id,
                        blocked_by_id,
                    }),
            )
            .collect();
        Ok(self.commit(
            TaskCommandResultData::uncommitted(snapshot, events),
            revision,
        ))
    }

    pub fn remove_dependency(
        &mut self,
        task_id: TaskIdData,
        blocked_by_id: TaskIdData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        let task = self
            .tasks
            .get(&task_id)
            .ok_or(TaskCommandError::TaskNotFound { id: task_id })?;
        if !self.tasks.contains_key(&blocked_by_id) {
            return Err(TaskCommandError::TaskNotFound { id: blocked_by_id }.into());
        }
        if !task.blocked_by().contains(&blocked_by_id) {
            return Ok(TaskCommandResultData::uncommitted(task.clone(), Vec::new()));
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
            TaskCommandResultData::uncommitted(
                snapshot,
                vec![TaskEventData::TaskDependencyRemoved {
                    task_id,
                    blocked_by_id,
                }],
            ),
            revision,
        ))
    }

    pub fn would_create_cycle(&self, task_id: TaskIdData, blocked_by_id: TaskIdData) -> bool {
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
        id: BatchIdData,
    ) -> Result<TaskCommandResultData<BatchData>, share::error::DomainError> {
        let batch = self
            .batches
            .get(&id)
            .ok_or(TaskCommandError::BatchNotFound { id })?;
        let mut dry_run = batch.clone();
        dry_run.transition_to(BatchStatusData::Paused)?;
        let revision = self.reserve_revision()?;
        let batch = self
            .batches
            .get_mut(&id)
            .expect("validated batch must exist");
        batch
            .transition_to(BatchStatusData::Paused)
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
            TaskCommandResultData::uncommitted(snapshot, Vec::new()),
            revision,
        ))
    }

    pub fn resume_batch(
        &mut self,
        id: BatchIdData,
    ) -> Result<TaskCommandResultData<BatchData>, share::error::DomainError> {
        let batch = self
            .batches
            .get(&id)
            .ok_or(TaskCommandError::BatchNotFound { id })?;
        if let Some(active) = self.current_batch {
            if active != id {
                return Err(TaskCommandError::ActiveBatchConflict {
                    active,
                    requested: id,
                }
                .into());
            }
        }
        let mut dry_run = batch.clone();
        dry_run.transition_to(BatchStatusData::Active)?;
        let revision = self.reserve_revision()?;
        let batch = self
            .batches
            .get_mut(&id)
            .expect("validated batch must exist");
        batch
            .transition_to(BatchStatusData::Active)
            .expect("legality pre-validated above");
        self.current_batch = Some(id);
        let snapshot = self
            .batches
            .get(&id)
            .expect("validated batch must exist")
            .clone();
        Ok(self.commit(
            TaskCommandResultData::uncommitted(snapshot, Vec::new()),
            revision,
        ))
    }

    pub fn archive_batch(
        &mut self,
        id: BatchIdData,
    ) -> Result<TaskCommandResultData<BatchData>, share::error::DomainError> {
        let batch = self
            .batches
            .get(&id)
            .ok_or(TaskCommandError::BatchNotFound { id })?;
        // Archiving is a terminal, idempotent transition: repeat calls on an
        // already-archived batch are a true no-op and must never reserve a
        // revision, so they keep succeeding even once the revision counter
        // is exhausted.
        if batch.status() == BatchStatusData::Archived {
            return Ok(TaskCommandResultData::uncommitted(
                batch.clone(),
                Vec::new(),
            ));
        }
        let mut dry_run = batch.clone();
        dry_run.transition_to(BatchStatusData::Archived)?;
        let revision = self.reserve_revision()?;
        let batch = self
            .batches
            .get_mut(&id)
            .expect("validated batch must exist");
        batch
            .transition_to(BatchStatusData::Archived)
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
            TaskCommandResultData::uncommitted(snapshot, Vec::new()),
            revision,
        ))
    }

    /// Runtime calls this once per BatchData at the end of every turn to atomically
    /// update `last_active_turn` / `silence_turns`; `active` reports whether the
    /// turn produced any activity for this BatchData. Only an `Active` batch may be
    /// updated; `Paused`/`Archived` batches return a typed error and are left
    /// completely unchanged. Calls that would not change any observable field
    /// are idempotent no-ops and never advance the revision.
    pub fn record_batch_turn(
        &mut self,
        id: BatchIdData,
        turn: u64,
        active: bool,
    ) -> Result<TaskCommandResultData<BatchData>, share::error::DomainError> {
        let batch = self
            .batches
            .get(&id)
            .ok_or(TaskCommandError::BatchNotFound { id })?;
        let mut dry_run = batch.clone();
        let changed = dry_run.record_turn(turn, active)?;
        if !changed {
            return Ok(TaskCommandResultData::uncommitted(dry_run, Vec::new()));
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
            TaskCommandResultData::uncommitted(snapshot, Vec::new()),
            revision,
        ))
    }

    pub fn is_blocked(&self, id: TaskIdData) -> Result<bool, share::error::DomainError> {
        Ok(!self.blocking_ids(id)?.is_empty())
    }

    pub(crate) fn blocking_ids(
        &self,
        id: TaskIdData,
    ) -> Result<Vec<TaskIdData>, share::error::DomainError> {
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
                        TaskStatusData::Completed | TaskStatusData::Deleted
                    )
                })
            })
            .collect())
    }

    pub fn transition_with_progress(
        &mut self,
        id: TaskIdData,
        to: TaskStatusData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<super::TaskProgressSnapshotData>, share::error::DomainError>
    {
        if to == TaskStatusData::InProgress {
            let blocked_by = self.blocking_ids(id)?;
            if !blocked_by.is_empty() {
                return Err(TaskCommandError::TaskBlocked { id, blocked_by }.into());
            }
        }
        let current = self
            .tasks
            .get(&id)
            .ok_or(TaskCommandError::TaskNotFound { id })?;
        let batch_id = current.batch();
        let mut dry_run = self.clone();
        let task = dry_run
            .tasks
            .get_mut(&id)
            .expect("validated task must exist");
        let task_result = if task.status() == TaskStatusData::Completed
            && matches!(to, TaskStatusData::Pending | TaskStatusData::InProgress)
        {
            task.reopen_from_completed(to, updated_at)?
        } else {
            task.transition_to(to, updated_at)?
        };

        let mut auto_reopened = false;
        let batch_status = dry_run
            .batches
            .get(&batch_id)
            .ok_or(TaskCommandError::BatchNotFound { id: batch_id })?
            .status();
        if batch_status == BatchStatusData::Archived
            && matches!(to, TaskStatusData::Pending | TaskStatusData::InProgress)
        {
            if let Some(active) = dry_run.current_batch {
                if active != batch_id {
                    return Err(TaskCommandError::ActiveBatchConflict {
                        active,
                        requested: batch_id,
                    }
                    .into());
                }
            }
            dry_run
                .batches
                .get_mut(&batch_id)
                .expect("validated batch must exist")
                .reopen()?;
            dry_run.current_batch = Some(batch_id);
            auto_reopened = true;
        }

        let unfinished = dry_run.tasks.values().any(|task| {
            task.batch() == batch_id
                && matches!(
                    task.status(),
                    TaskStatusData::Pending | TaskStatusData::InProgress
                )
        });
        let mut auto_closed = false;
        if !unfinished
            && dry_run
                .batches
                .get(&batch_id)
                .is_some_and(|batch| batch.status() == BatchStatusData::Active)
        {
            dry_run
                .batches
                .get_mut(&batch_id)
                .expect("validated batch must exist")
                .transition_to(BatchStatusData::Archived)?;
            if dry_run.current_batch == Some(batch_id) {
                dry_run.current_batch = None;
            }
            auto_closed = true;
        }

        let revision = self.reserve_revision()?;
        let progress = dry_run
            .progress_snapshot(batch_id, id, auto_closed, auto_reopened)
            .expect("validated task and batch must produce progress");
        let events = task_result.events;
        *self = dry_run;
        Ok(self.commit(
            TaskCommandResultData::uncommitted(progress, events),
            revision,
        ))
    }

    pub fn transition(
        &mut self,
        id: TaskIdData,
        to: TaskStatusData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        if to == TaskStatusData::InProgress {
            let blocked_by = self.blocking_ids(id)?;
            if !blocked_by.is_empty() {
                return Err(TaskCommandError::TaskBlocked { id, blocked_by }.into());
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

    pub fn delete_with_progress(
        &mut self,
        id: TaskIdData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<super::TaskProgressSnapshotData>, share::error::DomainError>
    {
        let batch_id = self
            .tasks
            .get(&id)
            .ok_or(TaskCommandError::TaskNotFound { id })?
            .batch();
        let revision = self.reserve_revision()?;
        let mut dry_run = self.clone();
        let task_result = dry_run.delete(id, updated_at)?;
        let unfinished = dry_run.tasks.values().any(|task| {
            task.batch() == batch_id
                && matches!(
                    task.status(),
                    TaskStatusData::Pending | TaskStatusData::InProgress
                )
        });
        let mut auto_closed = false;
        if !unfinished
            && dry_run
                .batches
                .get(&batch_id)
                .is_some_and(|batch| batch.status() == BatchStatusData::Active)
        {
            dry_run
                .batches
                .get_mut(&batch_id)
                .expect("validated batch must exist")
                .transition_to(BatchStatusData::Archived)?;
            if dry_run.current_batch == Some(batch_id) {
                dry_run.current_batch = None;
            }
            auto_closed = true;
        }
        let progress = dry_run
            .progress_snapshot(batch_id, id, auto_closed, false)
            .expect("validated task and batch must produce progress");
        let events = task_result.events;
        *self = dry_run;
        Ok(self.commit(
            TaskCommandResultData::uncommitted(progress, events),
            revision,
        ))
    }

    /// Removes all incoming/outgoing dependency edges and marks the TaskData
    /// `Deleted` in one commit. Repeated delete of an already-`Deleted` TaskData
    /// is an idempotent no-op: it returns the current snapshot with empty
    /// `events` and `revision() == None`, and never reserves a new revision.
    pub fn delete(
        &mut self,
        id: TaskIdData,
        updated_at: u64,
    ) -> Result<TaskCommandResultData<TaskData>, share::error::DomainError> {
        let (blocked_by, blocks) = {
            let task = self
                .tasks
                .get(&id)
                .ok_or(TaskCommandError::TaskNotFound { id })?;
            if task.status() == TaskStatusData::Deleted {
                return Ok(TaskCommandResultData::uncommitted(task.clone(), Vec::new()));
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
            TaskCommandResultData::uncommitted(
                snapshot,
                vec![TaskEventData::TaskDeleted { task_id: id }],
            ),
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

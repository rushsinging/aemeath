use std::collections::{HashMap, HashSet};

use super::{
    Batch, BatchCreateSpec, BatchId, BatchStatus, Task, TaskCommandError, TaskCommandResult,
    TaskCreateSpec, TaskEvent, TaskId, TaskStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStoreState {
    tasks: HashMap<TaskId, Task>,
    batches: HashMap<BatchId, Batch>,
    next_task_id: TaskId,
    next_batch_id: BatchId,
    current_batch: Option<BatchId>,
}

impl TaskStoreState {
    pub fn empty() -> Self {
        Self {
            tasks: HashMap::new(),
            batches: HashMap::new(),
            next_task_id: TaskId::new(1),
            next_batch_id: BatchId::new(1),
            current_batch: None,
        }
    }
    pub fn tasks(&self) -> &HashMap<TaskId, Task> {
        &self.tasks
    }
    pub fn batches(&self) -> &HashMap<BatchId, Batch> {
        &self.batches
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

    pub fn create_batch(
        &mut self,
        spec: BatchCreateSpec,
        timestamp: u64,
    ) -> Result<TaskCommandResult<Batch>, TaskCommandError> {
        if let Some(active) = self.current_batch {
            return Err(TaskCommandError::ActiveBatchConflict {
                active,
                requested: self.next_batch_id,
            });
        }
        let id = self.next_batch_id;
        let batch = Batch::create(id, spec, timestamp);
        self.batches.insert(id, batch.clone());
        self.current_batch = Some(id);
        self.next_batch_id = BatchId::new(id.get() + 1);
        Ok(TaskCommandResult {
            value: batch,
            events: Vec::new(),
        })
    }

    pub fn create_task(
        &mut self,
        spec: TaskCreateSpec,
        timestamp: u64,
    ) -> Result<TaskCommandResult<Task>, TaskCommandError> {
        let batch = self.current_batch.ok_or(TaskCommandError::NoActiveBatch)?;
        let id = self.next_task_id;
        let result = Task::create(id, batch, spec, timestamp);
        self.tasks.insert(id, result.value.clone());
        self.next_task_id = TaskId::new(id.get() + 1);
        Ok(result)
    }

    pub fn add_dependency(
        &mut self,
        task_id: TaskId,
        blocked_by_id: TaskId,
        updated_at: u64,
    ) -> Result<TaskCommandResult<()>, TaskCommandError> {
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
            return Ok(TaskCommandResult {
                value: (),
                events: Vec::new(),
            });
        }
        if self.would_create_cycle(task_id, blocked_by_id) {
            return Err(TaskCommandError::DependencyCycle {
                task_id,
                blocked_by_id,
            });
        }
        self.tasks
            .get_mut(&task_id)
            .expect("validated task must exist")
            .add_blocked_by(blocked_by_id, updated_at);
        self.tasks
            .get_mut(&blocked_by_id)
            .expect("validated blocker must exist")
            .add_blocks(task_id, updated_at);
        Ok(TaskCommandResult {
            value: (),
            events: vec![TaskEvent::TaskDependencyAdded {
                task_id,
                blocked_by_id,
            }],
        })
    }

    pub fn remove_dependency(
        &mut self,
        task_id: TaskId,
        blocked_by_id: TaskId,
        updated_at: u64,
    ) -> Result<TaskCommandResult<()>, TaskCommandError> {
        if !self.tasks.contains_key(&task_id) {
            return Err(TaskCommandError::TaskNotFound { id: task_id });
        }
        if !self.tasks.contains_key(&blocked_by_id) {
            return Err(TaskCommandError::TaskNotFound { id: blocked_by_id });
        }
        let removed = self
            .tasks
            .get_mut(&task_id)
            .expect("validated task must exist")
            .remove_blocked_by(blocked_by_id, updated_at);
        if !removed {
            return Ok(TaskCommandResult {
                value: (),
                events: Vec::new(),
            });
        }
        self.tasks
            .get_mut(&blocked_by_id)
            .expect("validated blocker must exist")
            .remove_blocks(task_id, updated_at);
        Ok(TaskCommandResult {
            value: (),
            events: vec![TaskEvent::TaskDependencyRemoved {
                task_id,
                blocked_by_id,
            }],
        })
    }

    fn would_create_cycle(&self, task_id: TaskId, blocked_by_id: TaskId) -> bool {
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

    pub fn pause_batch(&mut self, id: BatchId) -> Result<Batch, TaskCommandError> {
        let batch = self
            .batches
            .get_mut(&id)
            .ok_or(TaskCommandError::BatchNotFound { id })?;
        batch.transition_to(BatchStatus::Paused)?;
        if self.current_batch == Some(id) {
            self.current_batch = None;
        }
        Ok(batch.clone())
    }

    pub fn resume_batch(&mut self, id: BatchId) -> Result<Batch, TaskCommandError> {
        if !self.batches.contains_key(&id) {
            return Err(TaskCommandError::BatchNotFound { id });
        }
        if let Some(active) = self.current_batch {
            if active != id {
                return Err(TaskCommandError::ActiveBatchConflict {
                    active,
                    requested: id,
                });
            }
        }
        let batch = self
            .batches
            .get_mut(&id)
            .expect("validated batch must exist");
        batch.transition_to(BatchStatus::Active)?;
        self.current_batch = Some(id);
        Ok(batch.clone())
    }

    pub fn archive_batch(&mut self, id: BatchId) -> Result<Batch, TaskCommandError> {
        let batch = self
            .batches
            .get_mut(&id)
            .ok_or(TaskCommandError::BatchNotFound { id })?;
        batch.transition_to(BatchStatus::Archived)?;
        if self.current_batch == Some(id) {
            self.current_batch = None;
        }
        Ok(batch.clone())
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
        self.tasks
            .get_mut(&id)
            .ok_or(TaskCommandError::TaskNotFound { id })?
            .transition_to(to, updated_at)
    }

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
            (task.blocked_by().to_vec(), task.blocks().to_vec())
        };
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
        Ok(TaskCommandResult {
            value: task.clone(),
            events: vec![TaskEvent::TaskDeleted { task_id: id }],
        })
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

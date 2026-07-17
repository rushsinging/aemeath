use std::collections::HashMap;

use super::{Batch, BatchId, Task, TaskId};

#[derive(Debug, Clone)]
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
}

impl Default for TaskStoreState {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_state_has_canonical_initial_values() {
        let state = TaskStoreState::empty();
        assert!(state.tasks().is_empty());
        assert!(state.batches().is_empty());
        assert_eq!(state.next_task_id(), TaskId::new(1));
        assert_eq!(state.next_batch_id(), BatchId::new(1));
        assert_eq!(state.current_batch(), None);
    }
}

pub mod lifecycle;
pub mod types;

// Re-export public types so that external crate paths like
// `aemeath_core::task::Task`, `TaskStore`, etc. keep working.
pub use types::{Batch, BatchStatus, Task, TaskPriority, TaskSnapshot, TaskStatus};

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct TaskStore {
    pub(crate) tasks: Arc<Mutex<HashMap<String, Task>>>,
    next_id: Arc<Mutex<u64>>,
    /// Monotonically increasing batch ID. Each `create()` call checks if a new
    /// turn has started (no non-completed tasks exist) and bumps the batch.
    current_batch: Arc<Mutex<u64>>,
    /// Batches metadata for lifecycle management.
    /// Indexed by batch id, maintained alongside tasks.
    pub(crate) batches: Arc<Mutex<Vec<Batch>>>,
    /// Turn counter: increments each time agent starts processing a new user message.
    pub(crate) turn_counter: Arc<Mutex<u64>>,
}

impl TaskStore {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
            current_batch: Arc::new(Mutex::new(0)),
            batches: Arc::new(Mutex::new(Vec::new())),
            turn_counter: Arc::new(Mutex::new(0)),
        }
    }

    async fn resolve_task_batch(&self) -> u64 {
        if let Some(active) = self.active_list().await {
            return active.id;
        }

        let (has_active, has_any) = {
            let tasks = self.tasks.lock().await;
            let has_any = !tasks.is_empty();
            let has_active = tasks
                .values()
                .any(|t| t.status != TaskStatus::Completed && t.status != TaskStatus::Deleted);
            (has_active, has_any)
        };
        let mut batch = self.current_batch.lock().await;
        if has_any && !has_active {
            *batch += 1;
        }
        *batch
    }

    /// Create a new task with all fields
    pub async fn create(
        &self,
        subject: String,
        description: String,
        active_form: Option<String>,
    ) -> Task {
        let id = {
            let mut next_id = self.next_id.lock().await;
            let id = next_id.to_string();
            *next_id += 1;
            id
            // next_id lock released here
        };

        let batch = self.resolve_task_batch().await;
        let now = types::default_timestamp();
        let task = Task {
            id: id.clone(),
            subject,
            description,
            status: TaskStatus::Pending,
            active_form,
            owner: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            priority: TaskPriority::default(),
            progress: 0,
            progress_message: None,
            created_at: now,
            updated_at: now,
            session_id: None,
            tags: Vec::new(),
            batch,
        };

        self.tasks.lock().await.insert(id, task.clone());
        task
    }

    /// Create a task with priority
    pub async fn create_with_priority(
        &self,
        subject: String,
        description: String,
        active_form: Option<String>,
        priority: TaskPriority,
    ) -> Task {
        let id = {
            let mut next_id = self.next_id.lock().await;
            let id = next_id.to_string();
            *next_id += 1;
            id
        };

        let batch = self.resolve_task_batch().await;
        let now = types::default_timestamp();
        let task = Task {
            id: id.clone(),
            subject,
            description,
            status: TaskStatus::Pending,
            active_form,
            owner: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            priority,
            progress: 0,
            progress_message: None,
            created_at: now,
            updated_at: now,
            session_id: None,
            tags: Vec::new(),
            batch,
        };

        self.tasks.lock().await.insert(id, task.clone());
        task
    }

    /// Get a task by ID (async)
    pub async fn get(&self, id: &str) -> Option<Task> {
        self.tasks.lock().await.get(id).cloned()
    }

    /// Update a task
    pub async fn update(&self, id: &str, f: impl FnOnce(&mut Task)) -> Option<Task> {
        let mut tasks = self.tasks.lock().await;
        if let Some(task) = tasks.get_mut(id) {
            f(task);
            Some(task.clone())
        } else {
            None
        }
    }

    /// List all tasks (async)
    pub async fn list(&self) -> Vec<Task> {
        let tasks = self.tasks.lock().await;
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| t.status != TaskStatus::Deleted)
            .cloned()
            .collect();
        // Sort by priority (urgent first), then by created_at
        result.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        result
    }

    /// List tasks by status (async)
    pub async fn list_by_status(&self, status: TaskStatus) -> Vec<Task> {
        self.list()
            .await
            .into_iter()
            .filter(|t| t.status == status)
            .collect()
    }

    /// List tasks by priority (async)
    pub async fn list_by_priority(&self, priority: TaskPriority) -> Vec<Task> {
        self.list()
            .await
            .into_iter()
            .filter(|t| t.priority == priority)
            .collect()
    }

    /// List tasks for a session (async)
    pub async fn list_by_session(&self, session_id: &str) -> Vec<Task> {
        self.list()
            .await
            .into_iter()
            .filter(|t| t.session_id.as_ref() == Some(&session_id.to_string()))
            .collect()
    }

    /// Delete a task (soft delete by setting status to Deleted, async for auto-save)
    pub async fn delete(&self, id: &str) -> bool {
        self.update(id, |t| t.status = TaskStatus::Deleted)
            .await
            .is_some()
    }

    /// Clear all tasks
    pub async fn clear(&self) {
        {
            let mut tasks = self.tasks.lock().await;
            tasks.clear();
        }
        // Release tasks lock before acquiring next_id lock
        {
            let mut next_id = self.next_id.lock().await;
            *next_id = 1;
        }
    }

    /// Take a snapshot of all non-deleted tasks for session persistence.
    pub async fn snapshot(&self) -> TaskSnapshot {
        let tasks = self.tasks.lock().await;
        let next_id = *self.next_id.lock().await;
        let current_batch = *self.current_batch.lock().await;
        let batches = self.batches.lock().await.clone();
        let tasks: Vec<Task> = tasks
            .values()
            .filter(|t| t.status != TaskStatus::Deleted)
            .cloned()
            .collect();
        TaskSnapshot {
            tasks,
            next_id,
            current_batch,
            batches,
        }
    }

    /// Restore tasks from a snapshot (e.g. on session resume).
    pub async fn restore(&self, snapshot: TaskSnapshot) {
        let mut tasks = self.tasks.lock().await;
        let mut next_id = self.next_id.lock().await;
        let mut batch = self.current_batch.lock().await;
        let mut batches = self.batches.lock().await;
        tasks.clear();
        for t in snapshot.tasks {
            tasks.insert(t.id.clone(), t);
        }
        *next_id = snapshot.next_id;
        *batch = snapshot.current_batch;
        *batches = snapshot.batches;
    }

    /// List tasks belonging to the latest batch only.
    /// This shows the current turn's task list, including completed ones,
    /// but hides tasks from previous turns.
    pub async fn list_current_batch(&self) -> Vec<Task> {
        let max_batch = {
            let batches = self.batches.lock().await;
            batches
                .iter()
                .filter(|batch| matches!(batch.status, BatchStatus::Active | BatchStatus::Paused))
                .map(|batch| batch.id)
                .max()
        };
        let Some(max_batch) = max_batch else {
            return Vec::new();
        };

        let tasks = self.tasks.lock().await;
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| t.batch == max_batch && t.status != TaskStatus::Deleted)
            .cloned()
            .collect();
        result.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        result
    }

    #[cfg(test)]
    pub(crate) async fn set_current_batch_for_test(&self, batch_id: u64) {
        *self.current_batch.lock().await = batch_id;
    }

    /// Clear all deleted tasks from memory
    pub async fn purge_deleted(&self) {
        let mut tasks = self.tasks.lock().await;
        tasks.retain(|_, t| t.status != TaskStatus::Deleted);
    }

    /// Get statistics (async)
    pub async fn stats(&self) -> TaskStoreStats {
        let tasks = self.tasks.lock().await;
        let total = tasks.len();
        let pending = tasks
            .values()
            .filter(|t| t.status == TaskStatus::Pending)
            .count();
        let in_progress = tasks
            .values()
            .filter(|t| t.status == TaskStatus::InProgress)
            .count();
        let completed = tasks
            .values()
            .filter(|t| t.status == TaskStatus::Completed)
            .count();
        let deleted = tasks
            .values()
            .filter(|t| t.status == TaskStatus::Deleted)
            .count();

        let by_priority = tasks
            .values()
            .filter(|t| t.status != TaskStatus::Deleted)
            .fold(HashMap::new(), |mut acc, t| {
                *acc.entry(t.priority).or_insert(0) += 1;
                acc
            });

        TaskStoreStats {
            total,
            pending,
            in_progress,
            completed,
            deleted,
            by_priority,
        }
    }

    // ── Batch lifecycle management ──

    /// Create an active task list for the current coherent user request.
    pub async fn create_list(&self, subject: String, summary: String) -> Batch {
        let batch_id = {
            let mut current_batch = self.current_batch.lock().await;
            let has_existing_current_list = {
                let batches = self.batches.lock().await;
                batches.iter().any(|batch| batch.id == *current_batch)
            };
            if has_existing_current_list {
                *current_batch += 1;
            }
            *current_batch
        };

        let mut batch = Batch::new(batch_id);
        batch.summary = Some(if summary.trim().is_empty() {
            subject
        } else {
            summary
        });

        let mut batches = self.batches.lock().await;
        for existing in batches.iter_mut() {
            if existing.status == BatchStatus::Active {
                existing.status = BatchStatus::Paused;
            }
        }
        if let Some(existing) = batches.iter_mut().find(|existing| existing.id == batch_id) {
            *existing = batch.clone();
        } else {
            batches.push(batch.clone());
        }
        batch
    }

    /// Return the current active task list, if one exists.
    pub async fn active_list(&self) -> Option<Batch> {
        let current_batch = *self.current_batch.lock().await;
        let batches = self.batches.lock().await;
        batches
            .iter()
            .find(|batch| batch.id == current_batch && batch.status == BatchStatus::Active)
            .cloned()
    }

    /// Complete the current active task list.
    pub async fn complete_list(&self) -> Option<Batch> {
        let current_batch = *self.current_batch.lock().await;
        let mut batches = self.batches.lock().await;
        let batch = batches
            .iter_mut()
            .find(|batch| batch.id == current_batch && batch.status == BatchStatus::Active)?;
        batch.status = BatchStatus::Archived;
        Some(batch.clone())
    }

    /// Return the current batch id.
    pub async fn current_batch(&self) -> u64 {
        *self.current_batch.lock().await
    }

    /// List active/paused task lists that still have pending or in-progress tasks.
    pub async fn lists_with_pending(&self) -> Vec<Batch> {
        let batches = self.batches.lock().await.clone();
        let tasks = self.tasks.lock().await;
        let mut result: Vec<Batch> = batches
            .into_iter()
            .filter(|batch| {
                matches!(batch.status, BatchStatus::Active | BatchStatus::Paused)
                    && tasks.values().any(|task| {
                        task.batch == batch.id
                            && matches!(task.status, TaskStatus::Pending | TaskStatus::InProgress)
                    })
            })
            .collect();
        result.sort_by_key(|batch| batch.id);
        result
    }

    /// Get or create a batch by id.
    pub async fn get_or_create_batch(&self, batch_id: u64) -> Batch {
        let mut batches = self.batches.lock().await;
        if let Some(b) = batches.iter().find(|b| b.id == batch_id) {
            b.clone()
        } else {
            let b = Batch::new(batch_id);
            batches.push(b.clone());
            b
        }
    }

    /// Update a batch's status.
    pub async fn set_batch_status(&self, batch_id: u64, status: BatchStatus) {
        let mut batches = self.batches.lock().await;
        if let Some(b) = batches.iter_mut().find(|b| b.id == batch_id) {
            b.status = status;
        }
    }

    /// Set batch status for all batches except the current one.
    pub async fn archive_old_batches(&self, except_batch: u64) {
        let mut batches = self.batches.lock().await;
        for b in batches.iter_mut() {
            if b.id != except_batch {
                b.status = BatchStatus::Archived;
            }
        }
    }

    /// Get batch by id.
    pub async fn get_batch(&self, batch_id: u64) -> Option<Batch> {
        let batches = self.batches.lock().await;
        batches.iter().find(|b| b.id == batch_id).cloned()
    }

    /// List all batches with their metadata.
    pub async fn list_batches(&self) -> Vec<Batch> {
        let batches = self.batches.lock().await;
        batches.clone()
    }

    /// Check if a batch has all tasks completed.
    pub async fn is_batch_completed(&self, batch_id: u64) -> bool {
        let tasks = self.tasks.lock().await;
        let matching: Vec<_> = tasks
            .values()
            .filter(|t| t.batch == batch_id && t.status != TaskStatus::Deleted)
            .collect();
        !matching.is_empty() && matching.iter().all(|t| t.status == TaskStatus::Completed)
    }

    /// Get the count of incomplete tasks in a batch.
    pub async fn incomplete_count(&self, batch_id: u64) -> usize {
        let tasks = self.tasks.lock().await;
        tasks
            .values()
            .filter(|t| {
                t.batch == batch_id
                    && t.status != TaskStatus::Completed
                    && t.status != TaskStatus::Deleted
            })
            .count()
    }

    /// Get IDs of incomplete tasks in a batch.
    pub async fn incomplete_task_ids(&self, batch_id: u64) -> Vec<String> {
        let tasks = self.tasks.lock().await;
        tasks
            .values()
            .filter(|t| {
                t.batch == batch_id
                    && t.status != TaskStatus::Completed
                    && t.status != TaskStatus::Deleted
            })
            .map(|t| t.id.clone())
            .collect()
    }

    /// Cancel all incomplete tasks in a batch.
    pub async fn cancel_batch(&self, batch_id: u64) {
        let mut tasks = self.tasks.lock().await;
        for t in tasks.values_mut() {
            if t.batch == batch_id
                && t.status != TaskStatus::Completed
                && t.status != TaskStatus::Deleted
            {
                t.status = TaskStatus::Deleted;
            }
        }
        self.set_batch_status(batch_id, BatchStatus::Archived).await;
    }

    /// Increment turn counter and update silence_turns for all active batches.
    pub async fn advance_turn(&self) -> u64 {
        let mut counter = self.turn_counter.lock().await;
        *counter += 1;
        let current_turn = *counter;

        let mut batches = self.batches.lock().await;
        for b in batches.iter_mut() {
            if b.status == BatchStatus::Active {
                b.silence_turns += 1;
                b.last_active_turn = current_turn;
            }
        }
        current_turn
    }

    /// Reset silence_turns for a batch (called when the batch gets activity).
    pub async fn reset_silence(&self, batch_id: u64) {
        let mut batches = self.batches.lock().await;
        if let Some(b) = batches.iter_mut().find(|b| b.id == batch_id) {
            b.silence_turns = 0;
        }
    }

    /// Get the previous batch id (one before current), if any.
    pub async fn previous_batch(&self) -> Option<u64> {
        let batches = self.batches.lock().await;
        if batches.len() < 2 {
            return None;
        }
        // Sort by id descending, return the second one
        let mut ids: Vec<u64> = batches.iter().map(|b| b.id).collect();
        ids.sort_unstable();
        ids.pop(); // remove largest (current)
        ids.pop() // second largest = previous
    }

    /// Get tasks in a specific batch that match statuses.
    pub async fn tasks_in_batch(&self, batch_id: u64, statuses: &[TaskStatus]) -> Vec<Task> {
        let tasks = self.tasks.lock().await;
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| t.batch == batch_id && statuses.contains(&t.status))
            .cloned()
            .collect();
        result.sort_by_key(|t| t.id.parse::<u64>().unwrap_or(u64::MAX));
        result
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Task store statistics
#[derive(Debug, Clone)]
pub struct TaskStoreStats {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub deleted: usize,
    pub by_priority: HashMap<TaskPriority, usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_current_batch_hides_archived_batch() {
        let store = TaskStore::new();
        let batch = store
            .create_list("finished".to_string(), "finished summary".to_string())
            .await;
        let task = store
            .create("done".to_string(), "done description".to_string(), None)
            .await;
        store
            .update(&task.id, |task| task.status = TaskStatus::Completed)
            .await;
        store.complete_list().await;
        store.set_current_batch_for_test(batch.id).await;

        let tasks = store.list_current_batch().await;

        assert!(tasks.is_empty());
    }
}

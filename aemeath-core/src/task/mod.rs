pub mod batch;
pub mod lifecycle;
pub mod list;
pub mod types;

// Re-export public types so that external crate paths like
// `aemeath_core::task::Task`, `TaskStore`, etc. keep working.
pub use types::{Batch, BatchStatus, Task, TaskPriority, TaskSnapshot, TaskStatus, TaskStoreStats};

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

        // Bump batch if all existing tasks are completed (new turn)
        let batch = {
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
        };

        let now = types::default_timestamp();
        self.get_or_create_batch(batch).await;
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

        // Bump batch if all existing tasks are completed (new turn)
        let batch = {
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
        };

        let now = types::default_timestamp();
        self.get_or_create_batch(batch).await;
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
        let tasks = self.tasks.lock().await;
        let max_batch = tasks.values().map(|t| t.batch).max().unwrap_or(0);
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
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

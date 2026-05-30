pub mod batch;
pub mod display;
pub mod list;
pub mod store;
pub mod types;

pub use types::{Batch, BatchStatus, Task, TaskPriority, TaskSnapshot, TaskStatus};

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct TaskStore {
    pub(crate) tasks: Arc<Mutex<HashMap<String, Task>>>,
    pub(crate) next_id: Arc<Mutex<u64>>,
    /// Monotonically increasing batch ID. Each `create()` call checks if a new
    /// turn has started (no non-completed tasks exist) and bumps the batch.
    pub(crate) current_batch: Arc<Mutex<u64>>,
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

    pub(crate) async fn resolve_task_batch(&self) -> u64 {
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

    pub(crate) async fn next_task_id(&self) -> String {
        let mut next_id = self.next_id.lock().await;
        let id = next_id.to_string();
        *next_id += 1;
        id
    }

    pub(crate) async fn build_task(
        &self,
        subject: String,
        description: String,
        active_form: Option<String>,
        priority: TaskPriority,
    ) -> Task {
        let id = self.next_task_id().await;
        let batch = self.resolve_task_batch().await;
        let now = types::default_timestamp();
        Task {
            id,
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
        }
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

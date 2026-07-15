use super::{BatchStatus, Task, TaskPriority, TaskStatus, TaskStore};

fn sort_tasks_by_priority_then_created(tasks: &mut [Task]) {
    tasks.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.created_at.cmp(&b.created_at))
    });
}

impl TaskStore {
    /// List all tasks (async)
    pub async fn list(&self) -> Vec<Task> {
        let tasks = self.tasks.lock().await;
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| t.status != TaskStatus::Deleted)
            .cloned()
            .collect();
        sort_tasks_by_priority_then_created(&mut result);
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
            .filter(|t| t.session_id.as_deref() == Some(session_id))
            .collect()
    }

    /// List tasks belonging to the latest batch only.
    /// Returns empty when the latest batch has been archived (completed / discarded).
    pub async fn list_current_batch(&self) -> Vec<Task> {
        let max_batch = {
            let tasks = self.tasks.lock().await;
            tasks.values().map(|t| t.batch).max().unwrap_or(0)
        };
        // Check whether the latest batch has been archived.
        // If so, no active tasks to display — return empty.
        {
            let batches = self.batches.lock().await;
            if batches
                .iter()
                .any(|b| b.id == max_batch && b.status == BatchStatus::Archived)
            {
                return Vec::new();
            }
        }
        let tasks = self.tasks.lock().await;
        let mut result: Vec<Task> = tasks
            .values()
            .filter(|t| t.batch == max_batch && t.status != TaskStatus::Deleted)
            .cloned()
            .collect();
        sort_tasks_by_priority_then_created(&mut result);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::task::types::{Batch, BatchStatus};
    use crate::business::task::TaskStore;

    async fn store_with_batch(id: u64, status: BatchStatus) -> TaskStore {
        let store = TaskStore::new();
        store.batches.lock().await.push(Batch {
            id,
            status,
            ..Batch::new(id, 0)
        });
        store
    }

    async fn add_task(store: &TaskStore, id: &str, batch: u64, status: TaskStatus) {
        let task = Task {
            id: id.to_string(),
            subject: format!("task-{id}"),
            description: String::new(),
            status,
            batch,
            ..Task {
                id: id.to_string(),
                subject: format!("task-{id}"),
                description: String::new(),
                status: TaskStatus::Pending,
                owner: None,
                blocked_by: vec![],
                priority: TaskPriority::Normal,
                created_at: 0,
                updated_at: 0,
                session_id: None,
                batch: 0,
            }
        };
        store.tasks.lock().await.insert(task.id.clone(), task);
    }

    #[tokio::test]
    async fn test_list_current_batch_empty_store() {
        let store = TaskStore::new();
        let result = store.list_current_batch().await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_list_current_batch_active_batch_with_tasks() {
        let store = store_with_batch(0, BatchStatus::Active).await;
        add_task(&store, "1", 0, TaskStatus::Pending).await;
        add_task(&store, "2", 0, TaskStatus::InProgress).await;
        add_task(&store, "3", 0, TaskStatus::Completed).await;
        let result = store.list_current_batch().await;
        assert_eq!(result.len(), 3);
    }

    #[tokio::test]
    async fn test_list_current_batch_archived_batch_returns_empty() {
        let store = store_with_batch(0, BatchStatus::Archived).await;
        add_task(&store, "1", 0, TaskStatus::Completed).await;
        add_task(&store, "2", 0, TaskStatus::Completed).await;
        let result = store.list_current_batch().await;
        assert!(result.is_empty(), "archived batch should return empty");
    }

    #[tokio::test]
    async fn test_list_current_batch_paused_batch_returns_tasks() {
        let store = store_with_batch(0, BatchStatus::Paused).await;
        add_task(&store, "1", 0, TaskStatus::Pending).await;
        add_task(&store, "2", 0, TaskStatus::InProgress).await;
        let result = store.list_current_batch().await;
        assert_eq!(result.len(), 2, "paused batch should still return tasks");
    }

    #[tokio::test]
    async fn test_list_current_batch_new_active_batch_after_archived() {
        // Simulate: batch 0 archived, batch 1 active with new tasks
        let store = store_with_batch(0, BatchStatus::Archived).await;
        add_task(&store, "1", 0, TaskStatus::Completed).await;
        store.batches.lock().await.push(Batch {
            id: 1,
            status: BatchStatus::Active,
            ..Batch::new(1, 0)
        });
        add_task(&store, "2", 1, TaskStatus::Pending).await;
        add_task(&store, "3", 1, TaskStatus::InProgress).await;
        let result = store.list_current_batch().await;
        assert_eq!(result.len(), 2, "should return only batch 1 tasks");
    }
}

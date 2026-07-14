use super::{Task, TaskPriority, TaskSnapshot, TaskStatus, TaskStore, TaskStoreStats};
use std::collections::HashMap;

impl TaskStore {
    /// Create a new task
    pub async fn create(&self, subject: String, description: String) -> Task {
        self.create_with_priority(subject, description, TaskPriority::default())
            .await
    }

    /// Create a task with priority
    pub async fn create_with_priority(
        &self,
        subject: String,
        description: String,
        priority: TaskPriority,
    ) -> Task {
        let task = self.build_task(subject, description, priority).await;
        self.tasks
            .lock()
            .await
            .insert(task.id.clone(), task.clone());
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

    /// Delete a task (soft delete by setting status to Deleted, async for auto-save)
    pub async fn delete(&self, id: &str) -> bool {
        self.update(id, |t| t.status = TaskStatus::Deleted)
            .await
            .is_some()
    }

    /// Clear all tasks
    pub async fn clear(&self) {
        self.tasks.lock().await.clear();
        *self.next_id.lock().await = 1;
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

    /// Clear all deleted tasks from memory
    pub async fn purge_deleted(&self) {
        let mut tasks = self.tasks.lock().await;
        tasks.retain(|_, t| t.status != TaskStatus::Deleted);
    }

    pub async fn is_blocked(&self, task: &Task) -> bool {
        let tasks_snapshot = self.tasks.lock().await.clone();
        for id in &task.blocked_by {
            if let Some(t) = tasks_snapshot.get(id) {
                if t.status != TaskStatus::Completed {
                    return true;
                }
            }
        }
        false
    }

    pub async fn would_create_cycle(&self, task: &Task, blocked_by_id: &str) -> bool {
        if task.id == blocked_by_id {
            return true;
        }

        let tasks_snapshot = self.tasks.lock().await.clone();
        let mut visited: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut stack: Vec<&str> = vec![blocked_by_id];

        while let Some(current_id) = stack.pop() {
            if current_id == task.id {
                return true;
            }
            if visited.contains(current_id) {
                continue;
            }
            visited.insert(current_id);

            if let Some(current_task) = tasks_snapshot.get(current_id) {
                for dep_id in &current_task.blocked_by {
                    stack.push(dep_id.as_str());
                }
            }
        }

        false
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::task::BatchStatus;

    #[tokio::test]
    async fn test_create_list_resets_task_ids_for_new_batch() {
        let store = TaskStore::new();
        store
            .create_list("first".to_string(), "first batch".to_string())
            .await;
        let first = store
            .create("first task".to_string(), "desc".to_string())
            .await;
        store
            .update(&first.id, |task| task.status = TaskStatus::Completed)
            .await;
        store.complete_list().await;

        store
            .create_list("second".to_string(), "second batch".to_string())
            .await;
        let second = store
            .create("second task".to_string(), "desc".to_string())
            .await;

        assert_eq!(second.id, "1");
    }

    #[tokio::test]
    async fn test_create_list_drops_archived_batch_tasks_before_reusing_ids() {
        let store = TaskStore::new();
        store
            .create_list("first".to_string(), "first batch".to_string())
            .await;
        let first = store
            .create("first task".to_string(), "desc".to_string())
            .await;
        store
            .update(&first.id, |task| task.status = TaskStatus::Completed)
            .await;
        store.complete_list().await;

        store
            .create_list("second".to_string(), "second batch".to_string())
            .await;
        let second = store
            .create("second task".to_string(), "desc".to_string())
            .await;
        let stored = store.get("1").await.expect("new task should use reused id");

        assert_eq!(second.id, "1");
        assert_eq!(stored.subject, "second task");
        assert_eq!(store.list().await.len(), 1);
    }

    #[tokio::test]
    async fn test_clear_resets_task_ids() {
        let store = TaskStore::new();
        let first = store
            .create("first task".to_string(), "desc".to_string())
            .await;
        assert_eq!(first.id, "1");

        store.clear().await;
        let second = store
            .create("second task".to_string(), "desc".to_string())
            .await;

        assert_eq!(second.id, "1");
    }

    #[tokio::test]
    async fn test_complete_list_keeps_existing_task_ids() {
        let store = TaskStore::new();
        store
            .create_list("first".to_string(), "first batch".to_string())
            .await;
        let first = store
            .create("first task".to_string(), "desc".to_string())
            .await;

        store.complete_list().await;
        let stored = store
            .get(&first.id)
            .await
            .expect("task should remain stored");

        assert_eq!(stored.id, "1");
        assert!(store.get("2").await.is_none());
        assert_eq!(store.list_batches().await[0].status, BatchStatus::Archived);
    }
}

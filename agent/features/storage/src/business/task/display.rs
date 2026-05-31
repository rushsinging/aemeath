//! Display number mapping for batch-local task IDs.
//!
//! The TUI shows batch-local display numbers (#1, #2, …) while the internal
//! `Task.id` is a globally incrementing string. This module provides the
//! bidirectional mapping so that tools (TaskUpdate, TaskCreate, TaskList)
//! can accept and output display numbers that match what the user sees.

use std::collections::HashMap;

use super::{BatchStatus, TaskStatus, TaskStore};

impl TaskStore {
    /// Resolve a display number (batch-local id) to the global task id.
    ///
    /// The display number is the 1-based position when tasks in the same batch
    /// are sorted by their numeric global id. This matches what TUI shows.
    ///
    /// Falls back to treating `input` as a direct global id if display
    /// resolution fails (backward compatibility).
    pub async fn resolve_display_id(&self, input: &str) -> Option<String> {
        // Try display number first
        if let Ok(display_num) = input.parse::<usize>() {
            if display_num > 0 {
                if let Some(global_id) = self.display_number_to_global(display_num).await {
                    return Some(global_id);
                }
            }
        }
        // Fallback: treat as global id directly
        if self.get(input).await.is_some() {
            return Some(input.to_string());
        }
        None
    }

    /// Map a global task id to its display number (1-based) within its batch.
    ///
    /// Returns `None` if the task doesn't exist or has been deleted.
    pub async fn get_display_number(&self, global_id: &str) -> Option<usize> {
        let task = self.get(global_id).await?;
        let tasks = self
            .tasks_in_batch(
                task.batch,
                &[
                    TaskStatus::Pending,
                    TaskStatus::InProgress,
                    TaskStatus::Completed,
                ],
            )
            .await;
        tasks.iter().position(|t| t.id == global_id).map(|i| i + 1)
    }

    /// Resolve an array of task id strings (may be display numbers or global
    /// ids) to global task ids. Unresolvable entries are silently skipped.
    pub async fn resolve_display_ids(&self, ids: &[String]) -> Vec<String> {
        let mut resolved = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(global_id) = self.resolve_display_id(id).await {
                resolved.push(global_id);
            }
        }
        resolved
    }

    /// Convert an array of global task ids to display number strings.
    /// Falls back to the original global id string if mapping fails.
    pub async fn to_display_ids(&self, global_ids: &[String]) -> Vec<String> {
        let mut result = Vec::with_capacity(global_ids.len());
        for id in global_ids {
            if let Some(num) = self.get_display_number(id).await {
                result.push(num.to_string());
            } else {
                result.push(id.clone());
            }
        }
        result
    }

    /// Get the display number string for a global task id, with fallback.
    pub async fn format_display_id(&self, global_id: &str) -> String {
        self.get_display_number(global_id)
            .await
            .map(|n| n.to_string())
            .unwrap_or_else(|| global_id.to_string())
    }

    /// Map a display number (1-based) to global task id in the active batch.
    async fn display_number_to_global(&self, display_num: usize) -> Option<String> {
        let batch_id = self.display_batch_id().await?;
        let tasks = self
            .tasks_in_batch(
                batch_id,
                &[
                    TaskStatus::Pending,
                    TaskStatus::InProgress,
                    TaskStatus::Completed,
                ],
            )
            .await;
        tasks.into_iter().nth(display_num - 1).map(|t| t.id)
    }

    /// Batch-get display numbers for all tasks in the current display batch.
    ///
    /// Returns a map from global task id to 1-based display number.
    /// Returns empty map if no active batch.
    pub async fn get_batch_display_map(&self) -> HashMap<String, usize> {
        let Some(batch_id) = self.display_batch_id().await else {
            return HashMap::new();
        };
        let tasks = self
            .tasks_in_batch(
                batch_id,
                &[
                    TaskStatus::Pending,
                    TaskStatus::InProgress,
                    TaskStatus::Completed,
                ],
            )
            .await;
        tasks
            .into_iter()
            .enumerate()
            .map(|(i, t)| (t.id, i + 1))
            .collect()
    }

    /// Get the batch id used for display number resolution.
    ///
    /// Uses the same logic as `list_current_batch`: find the latest
    /// non-archived batch from tasks.
    async fn display_batch_id(&self) -> Option<u64> {
        let max_batch = {
            let tasks = self.tasks.lock().await;
            tasks
                .values()
                .filter(|t| t.status != TaskStatus::Deleted)
                .map(|t| t.batch)
                .max()?
        };
        // Check if archived
        let batches = self.batches.lock().await;
        if batches
            .iter()
            .any(|b| b.id == max_batch && b.status == BatchStatus::Archived)
        {
            return None;
        }
        Some(max_batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::business::task::{Batch, Task, TaskPriority};

    async fn setup_store_with_batches() -> TaskStore {
        TaskStore::new()
    }

    async fn add_task(store: &TaskStore, id: &str, batch: u64, status: TaskStatus) {
        let task = Task {
            id: id.to_string(),
            subject: format!("task-{id}"),
            description: String::new(),
            status,
            batch,
            active_form: None,
            owner: None,
            blocked_by: vec![],
            blocks: vec![],
            priority: TaskPriority::Normal,
            progress: 0,
            progress_message: None,
            created_at: 0,
            updated_at: 0,
            session_id: None,
            tags: vec![],
        };
        store.tasks.lock().await.insert(task.id.clone(), task);
    }

    // --- resolve_display_id ---

    #[tokio::test]
    async fn test_resolve_display_id_local_number_in_current_batch() {
        let store = setup_store_with_batches().await;
        store.batches.lock().await.push(Batch::new(1, 0));
        // Batch 1 has tasks with global ids 8, 9, 10
        add_task(&store, "8", 1, TaskStatus::Pending).await;
        add_task(&store, "9", 1, TaskStatus::InProgress).await;
        add_task(&store, "10", 1, TaskStatus::Pending).await;

        // Display number 2 should resolve to global id "9"
        let result = store.resolve_display_id("2").await;
        assert_eq!(result, Some("9".to_string()));
    }

    #[tokio::test]
    async fn test_resolve_display_id_fallback_to_global_id() {
        let store = setup_store_with_batches().await;
        // No batch, but task with id "42" exists
        add_task(&store, "42", 0, TaskStatus::Pending).await;

        let result = store.resolve_display_id("42").await;
        assert_eq!(result, Some("42".to_string()));
    }

    #[tokio::test]
    async fn test_resolve_display_id_not_found() {
        let store = setup_store_with_batches().await;
        let result = store.resolve_display_id("999").await;
        assert!(result.is_none());
    }

    // --- get_display_number ---

    #[tokio::test]
    async fn test_get_display_number_within_batch() {
        let store = setup_store_with_batches().await;
        store.batches.lock().await.push(Batch::new(1, 0));
        add_task(&store, "8", 1, TaskStatus::Pending).await;
        add_task(&store, "9", 1, TaskStatus::InProgress).await;
        add_task(&store, "10", 1, TaskStatus::Pending).await;

        // Global id "8" → display number 1
        assert_eq!(store.get_display_number("8").await, Some(1));
        // Global id "9" → display number 2
        assert_eq!(store.get_display_number("9").await, Some(2));
        // Global id "10" → display number 3
        assert_eq!(store.get_display_number("10").await, Some(3));
    }

    #[tokio::test]
    async fn test_get_display_number_task_not_found() {
        let store = setup_store_with_batches().await;
        assert_eq!(store.get_display_number("999").await, None);
    }

    #[tokio::test]
    async fn test_get_display_number_excludes_deleted() {
        let store = setup_store_with_batches().await;
        store.batches.lock().await.push(Batch::new(1, 0));
        add_task(&store, "1", 1, TaskStatus::Pending).await;
        add_task(&store, "2", 1, TaskStatus::Deleted).await;
        add_task(&store, "3", 1, TaskStatus::InProgress).await;

        // "1" is display 1, "3" is display 2 (deleted "2" excluded)
        assert_eq!(store.get_display_number("1").await, Some(1));
        assert_eq!(store.get_display_number("3").await, Some(2));
        // Deleted task "2" has no display number
        assert_eq!(store.get_display_number("2").await, None);
    }

    // --- resolve_display_ids / to_display_ids ---

    #[tokio::test]
    async fn test_resolve_display_ids_batch() {
        let store = setup_store_with_batches().await;
        store.batches.lock().await.push(Batch::new(1, 0));
        add_task(&store, "5", 1, TaskStatus::Pending).await;
        add_task(&store, "6", 1, TaskStatus::Pending).await;

        let result = store
            .resolve_display_ids(&["1".to_string(), "2".to_string()])
            .await;
        assert_eq!(result, vec!["5", "6"]);
    }

    #[tokio::test]
    async fn test_to_display_ids_batch() {
        let store = setup_store_with_batches().await;
        store.batches.lock().await.push(Batch::new(1, 0));
        add_task(&store, "5", 1, TaskStatus::Pending).await;
        add_task(&store, "6", 1, TaskStatus::Pending).await;

        let result = store
            .to_display_ids(&["5".to_string(), "6".to_string()])
            .await;
        assert_eq!(result, vec!["1", "2"]);
    }

    // --- display_batch_id ---

    #[tokio::test]
    async fn test_display_batch_id_archived_returns_none() {
        let store = setup_store_with_batches().await;
        let mut batch = Batch::new(1, 0);
        batch.status = BatchStatus::Archived;
        store.batches.lock().await.push(batch);
        add_task(&store, "1", 1, TaskStatus::Completed).await;

        assert_eq!(store.display_batch_id().await, None);
    }

    #[tokio::test]
    async fn test_display_batch_id_active_returns_batch() {
        let store = setup_store_with_batches().await;
        store.batches.lock().await.push(Batch::new(1, 0));
        add_task(&store, "1", 1, TaskStatus::Pending).await;

        assert_eq!(store.display_batch_id().await, Some(1));
    }

    // --- format_display_id ---

    #[tokio::test]
    async fn test_format_display_id_with_mapping() {
        let store = setup_store_with_batches().await;
        store.batches.lock().await.push(Batch::new(1, 0));
        add_task(&store, "99", 1, TaskStatus::Pending).await;

        assert_eq!(store.format_display_id("99").await, "1");
    }

    #[tokio::test]
    async fn test_format_display_id_fallback() {
        let store = setup_store_with_batches().await;
        // No task exists, fallback to raw id
        assert_eq!(store.format_display_id("42").await, "42");
    }

    // --- cross-batch consistency ---

    #[tokio::test]
    async fn test_cross_batch_display_numbers_independent() {
        let store = setup_store_with_batches().await;
        // Batch 0 (archived)
        let mut b0 = Batch::new(0, 0);
        b0.status = BatchStatus::Archived;
        store.batches.lock().await.push(b0);
        add_task(&store, "1", 0, TaskStatus::Completed).await;
        add_task(&store, "2", 0, TaskStatus::Completed).await;

        // Batch 1 (active)
        store.batches.lock().await.push(Batch::new(1, 0));
        add_task(&store, "8", 1, TaskStatus::Pending).await;
        add_task(&store, "9", 1, TaskStatus::Pending).await;

        // get_display_number works per-batch
        assert_eq!(store.get_display_number("8").await, Some(1));
        assert_eq!(store.get_display_number("9").await, Some(2));

        // resolve_display_id uses current batch (1)
        assert_eq!(store.resolve_display_id("1").await, Some("8".to_string()));
        assert_eq!(store.resolve_display_id("2").await, Some("9".to_string()));
    }

    // --- get_batch_display_map ---

    #[tokio::test]
    async fn test_get_batch_display_map_empty() {
        let store = setup_store_with_batches().await;
        let map = store.get_batch_display_map().await;
        assert!(map.is_empty());
    }

    #[tokio::test]
    async fn test_get_batch_display_map_returns_sequential_numbers() {
        let store = setup_store_with_batches().await;
        store.batches.lock().await.push(Batch::new(1, 0));
        add_task(&store, "8", 1, TaskStatus::Pending).await;
        add_task(&store, "9", 1, TaskStatus::InProgress).await;
        add_task(&store, "10", 1, TaskStatus::Completed).await;
        let map = store.get_batch_display_map().await;
        assert_eq!(map["8"], 1);
        assert_eq!(map["9"], 2);
        assert_eq!(map["10"], 3);
    }

    #[tokio::test]
    async fn test_get_batch_display_map_excludes_deleted() {
        let store = setup_store_with_batches().await;
        store.batches.lock().await.push(Batch::new(1, 0));
        add_task(&store, "1", 1, TaskStatus::Pending).await;
        add_task(&store, "2", 1, TaskStatus::Deleted).await;
        add_task(&store, "3", 1, TaskStatus::InProgress).await;
        let map = store.get_batch_display_map().await;
        assert_eq!(map.len(), 2);
        assert_eq!(map["1"], 1);
        assert_eq!(map["3"], 2);
    }
}

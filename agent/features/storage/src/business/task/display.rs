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
#[path = "display_tests.rs"]
mod display_tests;

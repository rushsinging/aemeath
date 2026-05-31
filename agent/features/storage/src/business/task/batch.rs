use super::{Batch, BatchStatus, Task, TaskStatus, TaskStore};

fn is_incomplete(status: TaskStatus) -> bool {
    status != TaskStatus::Completed && status != TaskStatus::Deleted
}

impl TaskStore {
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
                self.drop_archived_batch_tasks().await;
                *self.next_id.lock().await = 1;
            }
            *current_batch
        };

        let mut batch = Batch::new(batch_id, super::types::default_timestamp());
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

    async fn drop_archived_batch_tasks(&self) {
        let archived_batch_ids: std::collections::HashSet<u64> = {
            let batches = self.batches.lock().await;
            batches
                .iter()
                .filter(|batch| batch.status == BatchStatus::Archived)
                .map(|batch| batch.id)
                .collect()
        };

        if archived_batch_ids.is_empty() {
            return;
        }

        let mut tasks = self.tasks.lock().await;
        tasks.retain(|_, task| !archived_batch_ids.contains(&task.batch));
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
            let b = Batch::new(batch_id, super::types::default_timestamp());
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
            .filter(|t| t.batch == batch_id && is_incomplete(t.status.clone()))
            .count()
    }

    /// Get IDs of incomplete tasks in a batch.
    pub async fn incomplete_task_ids(&self, batch_id: u64) -> Vec<String> {
        let tasks = self.tasks.lock().await;
        tasks
            .values()
            .filter(|t| t.batch == batch_id && is_incomplete(t.status.clone()))
            .map(|t| t.id.clone())
            .collect()
    }

    /// Cancel all incomplete tasks in a batch.
    pub async fn cancel_batch(&self, batch_id: u64) {
        let mut tasks = self.tasks.lock().await;
        for t in tasks.values_mut() {
            if t.batch == batch_id && is_incomplete(t.status.clone()) {
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
        let mut ids: Vec<u64> = batches.iter().map(|b| b.id).collect();
        ids.sort_unstable();
        ids.pop();
        ids.pop()
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

use super::{Batch, BatchStatus, TaskStatus, TaskStore};

impl TaskStore {
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
        let mut ids: Vec<u64> = batches.iter().map(|b| b.id).collect();
        ids.sort_unstable();
        ids.pop();
        ids.pop()
    }
}

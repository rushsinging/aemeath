use super::{Batch, BatchStatus, Task, TaskStatus, TaskStore};

impl TaskStore {
    /// Ensure the current batch metadata exists.
    pub(crate) async fn ensure_current_batch(&self) -> u64 {
        let batch_id = *self.current_batch.lock().await;
        self.get_or_create_batch(batch_id).await;
        batch_id
    }

    /// Create a task list by assigning a summary to the current batch.
    pub async fn create_list(&self, subject: String, summary: String) -> Batch {
        let batch_id = self.ensure_current_batch().await;
        let mut batches = self.batches.lock().await;
        for batch in batches.iter_mut() {
            if batch.id != batch_id && batch.status == BatchStatus::Active {
                batch.status = BatchStatus::Paused;
            }
        }
        let batch = batches
            .iter_mut()
            .find(|batch| batch.id == batch_id)
            .expect("current batch exists");
        batch.status = BatchStatus::Active;
        batch.summary = Some(if summary.trim().is_empty() {
            subject
        } else {
            summary
        });
        batch.clone()
    }

    /// Complete the current task list/batch.
    pub async fn complete_list(&self) -> Option<Batch> {
        let batch_id = *self.current_batch.lock().await;
        let mut batches = self.batches.lock().await;
        let batch = batches.iter_mut().find(|batch| batch.id == batch_id)?;
        batch.status = BatchStatus::Archived;
        Some(batch.clone())
    }

    /// Return the active task list/batch, if any.
    pub async fn active_list(&self) -> Option<Batch> {
        let batches = self.batches.lock().await;
        batches
            .iter()
            .find(|batch| batch.status == BatchStatus::Active)
            .cloned()
    }

    /// List batches that still have incomplete tasks.
    pub async fn lists_with_pending(&self) -> Vec<Batch> {
        let batches = self.batches.lock().await.clone();
        let mut result = Vec::new();
        for batch in batches {
            if self.incomplete_count(batch.id).await > 0 {
                result.push(batch);
            }
        }
        result.sort_by_key(|batch| batch.id);
        result
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

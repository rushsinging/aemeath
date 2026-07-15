//! Scheduler persistence (save/restore) logic

use super::types::SchedulerState;
use super::TaskScheduler;
use storage::api::TaskStatus;

impl TaskScheduler {
    /// Persist state to disk
    pub async fn persist(&self) -> Result<(), String> {
        if !self.config.enable_persistence {
            return Ok(());
        }

        let path = self
            .config
            .persistence_path
            .clone()
            .unwrap_or_else(|| "task_scheduler_state.json".to_string());

        // Acquire locks one at a time and clone data, then release
        let active_tasks = self.active_tasks.lock().await.clone();
        let task_queue = self.task_queue.lock().await.clone();
        let execution_history = self.execution_history.lock().await.clone();

        let state = SchedulerState {
            active_tasks,
            task_queue,
            execution_history,
        };

        let json =
            serde_json::to_string(&state).map_err(|e| format!("Failed to serialize: {}", e))?;

        tokio::fs::write(&path, json)
            .await
            .map_err(|e| format!("Failed to write file: {}", e))?;

        Ok(())
    }

    /// Restore state from disk
    pub async fn restore(&self) -> Result<(), String> {
        if !self.config.enable_persistence {
            return Ok(());
        }

        let path = self
            .config
            .persistence_path
            .clone()
            .unwrap_or_else(|| "task_scheduler_state.json".to_string());

        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(());
        }

        let json = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?;

        let state: SchedulerState =
            serde_json::from_str(&json).map_err(|e| format!("Failed to deserialize: {}", e))?;

        *self.active_tasks.lock().await = state.active_tasks;
        *self.task_queue.lock().await = state.task_queue;
        *self.execution_history.lock().await = state.execution_history;

        // Collect task IDs to re-queue, then update task store without holding locks
        let task_ids: Vec<String> = {
            let active = self.active_tasks.lock().await;
            active.keys().cloned().collect()
        }; // active_tasks lock released

        for task_id in &task_ids {
            self.task_store
                .update(task_id, |t| {
                    t.status = TaskStatus::Pending;
                })
                .await;
        }

        Ok(())
    }
}

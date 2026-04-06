//! Background task scheduler and recovery system
//!
//! Provides:
//! - Background execution of tasks
//! - Task persistence and recovery
//! - Task queue management

use crate::task::{TaskStatus, TaskStore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use std::time::Duration;

/// Background task execution context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundTaskContext {
    /// Task ID
    pub task_id: String,
    /// Agent ID running this task (if applicable)
    pub agent_id: Option<String>,
    /// Start time
    pub started_at: u64,
    /// Last heartbeat time
    pub last_heartbeat: u64,
    /// Current progress
    pub progress: f32,
    /// Status message
    pub status_message: String,
    /// Can be interrupted
    pub interruptible: bool,
    /// Retry count
    pub retry_count: u32,
    /// Max retries allowed
    pub max_retries: u32,
}

/// Task execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionResult {
    /// Task ID
    pub task_id: String,
    /// Success or failure
    pub success: bool,
    /// Result output
    pub output: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
    /// Execution duration in seconds
    pub duration_seconds: u64,
    /// Resources used (token count, etc.)
    pub resources_used: HashMap<String, u64>,
}

/// Task scheduler configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Maximum concurrent background tasks
    pub max_concurrent: usize,
    /// Task timeout in seconds
    pub task_timeout_seconds: u64,
    /// Heartbeat interval in seconds
    pub heartbeat_interval_seconds: u64,
    /// Retry delay in seconds
    pub retry_delay_seconds: u64,
    /// Enable persistence
    pub enable_persistence: bool,
    /// Persistence path
    pub persistence_path: Option<String>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 5,
            task_timeout_seconds: 300,
            heartbeat_interval_seconds: 30,
            retry_delay_seconds: 10,
            enable_persistence: true,
            persistence_path: None,
        }
    }
}

/// Background task scheduler
#[derive(Debug)]
pub struct TaskScheduler {
    /// Task store reference
    task_store: Arc<TaskStore>,
    /// Active background tasks
    active_tasks: Arc<Mutex<HashMap<String, BackgroundTaskContext>>>,
    /// Task queue (pending tasks waiting to run)
    task_queue: Arc<Mutex<Vec<String>>>,
    /// Configuration
    config: SchedulerConfig,
    /// Notification for new tasks
    task_available: Arc<Notify>,
    /// Shutdown flag
    shutdown: Arc<Mutex<bool>>,
    /// Execution history
    execution_history: Arc<Mutex<HashMap<String, Vec<TaskExecutionResult>>>>,
}

impl TaskScheduler {
    /// Create a new task scheduler
    pub fn new(task_store: Arc<TaskStore>) -> Self {
        Self {
            task_store,
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            task_queue: Arc::new(Mutex::new(Vec::new())),
            config: SchedulerConfig::default(),
            task_available: Arc::new(Notify::new()),
            shutdown: Arc::new(Mutex::new(false)),
            execution_history: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create with custom configuration
    pub fn with_config(task_store: Arc<TaskStore>, config: SchedulerConfig) -> Self {
        Self {
            task_store,
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            task_queue: Arc::new(Mutex::new(Vec::new())),
            config,
            task_available: Arc::new(Notify::new()),
            shutdown: Arc::new(Mutex::new(false)),
            execution_history: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Queue a task for background execution
    pub async fn queue_task(&self, task_id: String) -> Result<(), String> {
        // Verify task exists and is in Pending status
        let task = self.task_store.get(&task_id).await;
        let task = task.ok_or_else(|| format!("Task {} not found", task_id))?;
        if task.status != TaskStatus::Pending {
            return Err(format!("Task {} is not in Pending status", task_id));
        }

        // Add to queue
        let mut queue = self.task_queue.lock().await;
        queue.push(task_id.clone());
        
        // Notify scheduler
        self.task_available.notify_one();
        
        Ok(())
    }

    /// Get the next task from queue
    pub async fn get_next_task(&self) -> Option<String> {
        let mut queue = self.task_queue.lock().await;
        if queue.is_empty() {
            return None;
        }
        
        // Find a task that's not blocked
        for i in 0..queue.len() {
            let task_id = queue[i].clone();
            if let Some(task) = self.task_store.get(&task_id).await {
                // Check if all blocking tasks are completed
                let mut all_blockers_done = true;
                for b in &task.blocked_by {
                    if let Some(blocker) = self.task_store.get(b).await {
                        if blocker.status != TaskStatus::Completed {
                            all_blockers_done = false;
                            break;
                        }
                    }
                }
                
                if all_blockers_done {
                    queue.remove(i);
                    return Some(task_id);
                }
            }
        }
        
        None
    }

    /// Start executing a task in background
    pub async fn start_task(&self, task_id: String, agent_id: Option<String>) -> Result<BackgroundTaskContext, String> {
        let mut active = self.active_tasks.lock().await;
        
        if active.len() >= self.config.max_concurrent {
            return Err("Maximum concurrent tasks reached".to_string());
        }

        // Update task status
        self.task_store.update(&task_id, |t| {
            t.status = TaskStatus::InProgress;
        }).await;

        let now = current_timestamp();
        let context = BackgroundTaskContext {
            task_id: task_id.clone(),
            agent_id,
            started_at: now,
            last_heartbeat: now,
            progress: 0.0,
            status_message: "Starting".to_string(),
            interruptible: true,
            retry_count: 0,
            max_retries: 3,
        };

        active.insert(task_id, context.clone());
        Ok(context)
    }

    /// Update task progress
    pub async fn update_progress(&self, task_id: &str, progress: f32, message: String) {
        let mut active = self.active_tasks.lock().await;
        if let Some(ctx) = active.get_mut(task_id) {
            ctx.progress = progress;
            ctx.status_message = message;
            ctx.last_heartbeat = current_timestamp();
        }
    }

    /// Complete a task
    pub async fn complete_task(&self, task_id: &str, result: TaskExecutionResult) {
        let mut active = self.active_tasks.lock().await;

        // Update task status
        if result.success {
            active.remove(task_id);
            self.task_store.update(task_id, |t| {
                t.status = TaskStatus::Completed;
            }).await;
        } else {
            // Check retry BEFORE removing from active
            let should_retry = if let Some(ctx) = active.get(task_id) {
                ctx.retry_count < ctx.max_retries
            } else {
                false
            };
            
            if should_retry {
                // Increment retry count and re-queue
                if let Some(ctx) = active.get_mut(task_id) {
                    ctx.retry_count += 1;
                }
                let mut queue = self.task_queue.lock().await;
                queue.push(task_id.to_string());
            } else {
                // No more retries — remove from active and mark failed
                active.remove(task_id);
                self.task_store.update(task_id, |t| {
                    t.status = TaskStatus::Pending; // Reset to pending (failed)
                }).await;
            }
        }

        // Record in history
        let mut history = self.execution_history.lock().await;
        history.entry(task_id.to_string())
            .or_insert_with(Vec::new)
            .push(result);

        // Notify for next task
        self.task_available.notify_one();
    }

    /// Cancel a running task
    pub async fn cancel_task(&self, task_id: &str) -> Result<(), String> {
        let mut active = self.active_tasks.lock().await;
        
        if let Some(ctx) = active.get(task_id) {
            if !ctx.interruptible {
                return Err("Task is not interruptible".to_string());
            }
        }

        active.remove(task_id);
        
        self.task_store.update(task_id, |t| {
            t.status = TaskStatus::Deleted;
        }).await;

        Ok(())
    }

    /// Get all active tasks
    pub async fn get_active_tasks(&self) -> Vec<BackgroundTaskContext> {
        let active = self.active_tasks.lock().await;
        active.values().cloned().collect()
    }

    /// Get queue length
    pub async fn queue_length(&self) -> usize {
        let queue = self.task_queue.lock().await;
        queue.len()
    }

    /// Check for timed out tasks
    pub async fn check_timeouts(&self) -> Vec<String> {
        let now = current_timestamp();
        let timeout_seconds = self.config.task_timeout_seconds;
        
        let active = self.active_tasks.lock().await;
        active.iter()
            .filter(|(_, ctx)| now - ctx.last_heartbeat > timeout_seconds)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Persist state to disk
    pub async fn persist(&self) -> Result<(), String> {
        if !self.config.enable_persistence {
            return Ok(());
        }

        let path = self.config.persistence_path.clone()
            .unwrap_or_else(|| "task_scheduler_state.json".to_string());

        let state = SchedulerState {
            active_tasks: self.active_tasks.lock().await.clone(),
            task_queue: self.task_queue.lock().await.clone(),
            execution_history: self.execution_history.lock().await.clone(),
        };

        let json = serde_json::to_string(&state)
            .map_err(|e| format!("Failed to serialize: {}", e))?;

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

        let path = self.config.persistence_path.clone()
            .unwrap_or_else(|| "task_scheduler_state.json".to_string());

        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(());
        }

        let json = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?;

        let state: SchedulerState = serde_json::from_str(&json)
            .map_err(|e| format!("Failed to deserialize: {}", e))?;

        *self.active_tasks.lock().await = state.active_tasks;
        *self.task_queue.lock().await = state.task_queue;
        *self.execution_history.lock().await = state.execution_history;

        // Re-queue active tasks that were interrupted
        let active = self.active_tasks.lock().await;
        for task_id in active.keys() {
            self.task_store.update(task_id, |t| {
                t.status = TaskStatus::Pending;
            }).await;
        }

        Ok(())
    }

    /// Start the scheduler loop
    pub async fn run(&self) {
        loop {
            // Check for shutdown
            if *self.shutdown.lock().await {
                break;
            }

            // Wait for task or timeout
            tokio::time::timeout(
                Duration::from_secs(self.config.heartbeat_interval_seconds),
                self.task_available.notified()
            ).await.ok();

            // Check for timeouts
            let timed_out = self.check_timeouts().await;
            for task_id in timed_out {
                if let Err(e) = self.cancel_task(&task_id).await {
                    log::warn!("Failed to cancel task {}: {}", task_id, e);
                }
            }

            // Persist state
            if let Err(e) = self.persist().await {
                log::warn!("Failed to persist scheduler state: {}", e);
            }
        }
    }

    /// Shutdown the scheduler
    pub async fn shutdown(&self) {
        *self.shutdown.lock().await = true;
        self.task_available.notify_one();
        
        // Persist final state
        if let Err(e) = self.persist().await {
            log::warn!("Failed to persist scheduler state on shutdown: {}", e);
        }
    }
}

/// Serializable scheduler state
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SchedulerState {
    active_tasks: HashMap<String, BackgroundTaskContext>,
    task_queue: Vec<String>,
    execution_history: HashMap<String, Vec<TaskExecutionResult>>,
}

/// Current timestamp in seconds
fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO)
        .as_secs()
}

/// Shared task scheduler
pub type SharedTaskScheduler = Arc<TaskScheduler>;

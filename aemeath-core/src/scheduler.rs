//! Background task scheduler and recovery system
//!
//! Provides:
//! - Background execution of tasks
//! - Task persistence and recovery
//! - Task queue management
//!
//! # Lock ordering
//!
//! To prevent deadlocks, the following lock ordering must be observed:
//! 1. `active_tasks` (RwLock) must be acquired BEFORE `task_queue` (Mutex)
//! 2. Never acquire `task_queue` while holding `active_tasks` in a way that
//!    could cause a circular wait with other tasks
//!
//! If you need both locks, always acquire `active_tasks` first, then `task_queue`.

use crate::task::{TaskStatus, TaskStore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
    shutdown: Arc<AtomicBool>,
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
            shutdown: Arc::new(AtomicBool::new(false)),
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
            shutdown: Arc::new(AtomicBool::new(false)),
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
        // Phase 1: Clone the queue so we don't hold the lock during await
        let queue_snapshot: Vec<String>;
        {
            let queue = self.task_queue.lock().await;
            queue_snapshot = queue.clone();
        } // task_queue lock released

        // Phase 2: Find an unblocked task (can safely await task_store)
        for (i, task_id) in queue_snapshot.iter().enumerate() {
            let task = self.task_store.get(task_id).await;
            let Some(task) = task else { continue };

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
                // Remove from queue and return
                let mut queue = self.task_queue.lock().await;
                queue.remove(i);
                return Some(task_id.clone());
            }
        }
        
        None
    }

    /// Start executing a task in background
    pub async fn start_task(&self, task_id: String, agent_id: Option<String>) -> Result<BackgroundTaskContext, String> {
        // Phase 1: Check capacity and compute context while holding active_tasks
        let context = {
            let mut active = self.active_tasks.lock().await;
        
            if active.len() >= self.config.max_concurrent {
                return Err("Maximum concurrent tasks reached".to_string());
            }

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
            context
        }; // active_tasks lock released

        // Phase 2: Update task store WITHOUT holding active_tasks
        self.task_store.update(&context.task_id, |t| {
            t.status = TaskStatus::InProgress;
        }).await;

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
        // --- Phase 1: Collect action under active_tasks lock, then release ---
        enum PostAction {
            Requeue,
            MarkCompleted,
            MarkFailed,
        }
        let post_action = {
            let mut active = self.active_tasks.lock().await;
  
            if result.success {
                active.remove(task_id);
                Some(PostAction::MarkCompleted)
            } else {
                // Check retry BEFORE removing from active
                let should_retry = if let Some(ctx) = active.get(task_id) {
                    ctx.retry_count < ctx.max_retries
                } else {
                    false
                };
  
                if should_retry {
                    if let Some(ctx) = active.get_mut(task_id) {
                        ctx.retry_count += 1;
                    }
                    Some(PostAction::Requeue)
                } else {
                    active.remove(task_id);
                    Some(PostAction::MarkFailed)
                }
            }
        }; // active_tasks lock released here

        // --- Phase 2: Update task store WITHOUT holding active_tasks ---
        match post_action {
            Some(PostAction::MarkCompleted) => {
                self.task_store.update(task_id, |t| {
                    t.status = TaskStatus::Completed;
                }).await;
            }
            Some(PostAction::MarkFailed) => {
                self.task_store.update(task_id, |t| {
                    t.status = TaskStatus::Pending;
                }).await;
            }
            _ => {}
        }

        // --- Phase 3: Operate on other locks ---
        if let Some(PostAction::Requeue) = post_action {
            let mut queue = self.task_queue.lock().await;
            queue.push(task_id.to_string());
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
        // Phase 1: Check and remove from active_tasks
        {
            let mut active = self.active_tasks.lock().await;
        
            if let Some(ctx) = active.get(task_id) {
                if !ctx.interruptible {
                    return Err("Task is not interruptible".to_string());
                }
            }

            active.remove(task_id);
        } // active_tasks lock released

        // Phase 2: Update task store WITHOUT holding active_tasks
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

        // Acquire locks one at a time and clone data, then release
        let active_tasks = self.active_tasks.lock().await.clone();
        let task_queue = self.task_queue.lock().await.clone();
        let execution_history = self.execution_history.lock().await.clone();

        let state = SchedulerState {
            active_tasks,
            task_queue,
            execution_history,
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

        // Collect task IDs to re-queue, then update task store without holding locks
        let task_ids: Vec<String> = {
            let active = self.active_tasks.lock().await;
            active.keys().cloned().collect()
        }; // active_tasks lock released

        for task_id in &task_ids {
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
            if self.shutdown.load(Ordering::Relaxed) {
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
        self.shutdown.store(true, Ordering::Relaxed);
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

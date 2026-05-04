//! Scheduler type definitions

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// Serializable scheduler state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SchedulerState {
    pub active_tasks: HashMap<String, BackgroundTaskContext>,
    pub task_queue: Vec<String>,
    pub execution_history: HashMap<String, Vec<TaskExecutionResult>>,
}

/// Current timestamp in seconds
pub(super) fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO)
        .as_secs()
}

use serde::{Deserialize, Serialize};

pub const DEFAULT_USAGE_QUEUE_CAPACITY: usize = 1024;
pub const DEFAULT_USAGE_SHUTDOWN_TIMEOUT_MS: u64 = 5_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    #[serde(default = "default_usage_queue_capacity")]
    pub usage_queue_capacity: usize,
    #[serde(default = "default_usage_shutdown_timeout_ms")]
    pub usage_shutdown_timeout_ms: u64,
}

pub const fn default_usage_queue_capacity() -> usize {
    DEFAULT_USAGE_QUEUE_CAPACITY
}

pub const fn default_usage_shutdown_timeout_ms() -> u64 {
    DEFAULT_USAGE_SHUTDOWN_TIMEOUT_MS
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            usage_queue_capacity: DEFAULT_USAGE_QUEUE_CAPACITY,
            usage_shutdown_timeout_ms: DEFAULT_USAGE_SHUTDOWN_TIMEOUT_MS,
        }
    }
}

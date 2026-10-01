//! Logging crate 生产常量（#1146 双轨归位）。

pub const LOG_MAX_BYTES: u64 = 100 * 1024 * 1024;
pub const LOG_MAX_BACKUPS: usize = 5;
pub const LOG_RETENTION_DAYS: u64 = 30;

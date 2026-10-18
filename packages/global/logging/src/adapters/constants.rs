//! Logging adapters 常量（#1146 双轨归位）。

use std::time::Duration;

/// 未登记 target 的告警限频。
pub(crate) const UNKNOWN_TARGET_REPORT_LIMIT: usize = 3;
/// 异步 sink channel 容量：远高于日志峰值速率，饱和时丢弃计数而非反压调用线程。
pub(crate) const ASYNC_SINK_CHANNEL_CAPACITY: usize = 8192;
pub(crate) const EMERGENCY_LOG_FILE: &str = "emergency.log";
pub(crate) const RECOVERY_INTERVAL: Duration = Duration::from_secs(5);
/// fd 常量仅 Unix 路由面消费（Windows 分支不读，放行跨目标 dead_code）。
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) const STDOUT_FD: i32 = 1;
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) const STDERR_FD: i32 = 2;

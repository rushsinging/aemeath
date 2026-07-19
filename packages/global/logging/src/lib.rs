//! # 诊断日志职责
//!
//! DiagnosticRecord 走 `log::log!` → `UnifiedLogger::log()` → `format_diag_json_line`。
//! Audit Event 与 Usage Fact 使用 Audit 自有契约和 sink，绝不进入该管线。
//!
//! ## 文件路由
//!
//! 合法 target、owner、sink ID 与文件名由私有 `domain::routing::TargetCatalog` 唯一定义；
//! `UnifiedLogger` 只消费 catalog。未知 target 写入 `aemeath.log`。emergency 兜底（sink
//! degrade / fallback）在 File 模式下写 `emergency.log`，**NEVER** 写 stderr——stderr 会
//! 越过 TUI alternate screen 的双缓冲直接糊屏（见 #1215）。
//! `panic.log` 由 panic hook 直写，不纳入 UnifiedLogger。

mod adapters;
mod domain;

pub use adapters::{
    app_version, boot_ts, capture, instrument, is_rotated_log_path, rotated_path, set_app_version,
    set_boot_ts, spawn_instrumented, timestamp_local_rfc3339, timestamp_rfc3339, within,
    UnifiedLogger,
};
pub use domain::{FieldPatch, LogContext, LogContextPatch, LoggingOutputMode, LoggingSettings};

/// 解析 `level` 字符串为 `log::LevelFilter`，解析失败时回退到 `Warn`。
pub fn level_filter_from_str(level: &str) -> log::LevelFilter {
    match level.to_ascii_lowercase().as_str() {
        "off" => log::LevelFilter::Off,
        "error" => log::LevelFilter::Error,
        "warn" | "warning" => log::LevelFilter::Warn,
        "info" => log::LevelFilter::Info,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        _ => log::LevelFilter::Warn,
    }
}

pub const LOG_MAX_BYTES: u64 = 100 * 1024 * 1024;
pub const LOG_MAX_BACKUPS: usize = 5;
pub const LOG_RETENTION_DAYS: u64 = 30;

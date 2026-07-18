//! # 日志文件职责
//!
//! 所有日志（诊断 + 审计）统一走 `log::log!` → `UnifiedLogger::log()` → `format_diag_json_line`。
//!
//! ## 文件路由
//!
//! 合法 target、owner、sink ID 与文件名由私有 `domain::routing::TargetCatalog` 唯一定义；
//! `UnifiedLogger` 只消费 catalog。未知 target 写入 `aemeath.log` 并限频报告到 stderr。
//! `panic.log` 由 panic hook 直写，不纳入 UnifiedLogger。

mod adapters;
mod domain;

pub use adapters::{
    app_version, boot_ts, capture, current_chat_id, current_model, current_provider,
    current_request_id, current_role, current_turn, instrument, is_rotated_log_path, rotated_path,
    session_id, set_app_version, set_boot_ts, set_current_chat_id, set_current_model,
    set_current_provider, set_current_request_id, set_current_role, set_current_turn,
    set_session_id, spawn_instrumented, timestamp_local_rfc3339, timestamp_rfc3339, within,
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

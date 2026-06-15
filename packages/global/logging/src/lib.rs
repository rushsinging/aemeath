//! # 日志文件职责
//!
//! ## 诊断日志（UnifiedLogger 按 target 前缀路由）
//!
//! | 文件 | target 前缀 | 来源 |
//! |------|-------------|------|
//! | `aemeath.log` | 兜底 | shared/composition + 其他 |
//! | `runtime.log` | `runtime::` | runtime crate |
//! | `provider.log` | `provider::` | provider crate |
//! | `tools.log` | `tools::` | tools crate + tool_call/tool_result |
//! | `prompt.log` | `prompt::` | prompt crate |
//! | `tui.log` | `cli::` | cli/tui |
//! | `hook.log` | `hook::` | hook crate |
//!
//! ## 原始记录（静态方法直写）
//!
//! | 文件 | 数据 | 写入方法 |
//! |------|------|----------|
//! | `input.log` | 用户输入 + LLM 输入 | `log_input` / `log_user_input` |
//! | `output.log` | LLM 输出 | `log_output` |
//!
//! ## 审计
//!
//! | 文件 | 数据 | 写入方法 |
//! |------|------|----------|
//! | `audit.log` | 权限/行为审计（预留） | `audit` |
//!
//! ## 不变
//!
//! | 文件 | 说明 |
//! |------|------|
//! | `panic.log` | panic_hook.rs 直写，不纳入 UnifiedLogger |

pub mod context;
pub mod format;
pub mod rotation;
pub mod target_guard;
pub mod text;
pub mod unified_logger;

pub use context::{
    current_chat_id, current_model, current_turn, session_id, set_current_chat_id,
    set_current_model, set_current_turn, set_session_id,
};
pub use rotation::{is_rotated_log_path, rotated_path, timestamp_rfc3339};
pub use text::{
    append_json_line, append_json_line_with_turn, append_line, append_text_line,
    append_text_line_with_turn, format_text_line, format_text_line_with_turn, open_append,
    prepare_log_file, LogFile,
};
pub use unified_logger::UnifiedLogger;

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

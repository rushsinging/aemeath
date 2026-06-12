//! 日志文件管理与格式化输出
//!
//! 路径无关：所有接受文件路径的函数通过 `base_dir: &Path` 参数传入。
//!
//! # 日志文件职责（feature #79 路径 C）
//!
//! | 文件 | 职责 | 写入路径 |
//! |------|------|----------|
//! | `aemeath.log` | 应用主日志（兜底） | `UnifiedLogger::log` 中 `target` 不以 `cli::`/`hook::` 开头 |
//! | `tui.log` | TUI 渲染/事件/状态 | `UnifiedLogger::log` 中 `target` 以 `cli::` 开头 |
//! | `hook.log` | Hook 匹配/执行/结果 | `UnifiedLogger::log` 中 `target` 以 `hook::` 开头 |
//! | `input.log` | LLM 输入快照 | `UnifiedLogger::log_input` 静态方法 |
//! | `output.log` | LLM 完整输出 | `UnifiedLogger::log_output` 静态方法 |
//! | `tool.log` | 工具调用请求 + 执行结果 | `UnifiedLogger::log_tool` 静态方法 |
//! | `panic.log` | Panic 崩溃日志 | `panic_hook.rs`，不纳入 `UnifiedLogger` |

pub mod context;
pub mod format;
pub mod rotation;
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
pub use unified_logger::{ToolKind, UnifiedLogger};

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

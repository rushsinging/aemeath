//! # 日志文件职责
//!
//! ## 诊断日志（UnifiedLogger 按 target 前缀路由）
//!
//! | 文件 | target 前缀 | 来源 |
//! |------|-------------|------|
//! | `aemeath.log` | 兜底 | shared/composition + 其他 |
//! | `tui.log` | `aemeath:tui` | cli/tui |
//! | `shared.log` | `aemeath:shared` | shared 层 |
//! | `composition.log` | `aemeath:composition` | composition 层 |
//! | `agent-provider.log` | `aemeath:agent:provider` | provider crate + LLM 输入/输出 |
//! | `agent-runtime.log` | `aemeath:agent:runtime` | runtime crate |
//! | `agent-tools.log` | `aemeath:agent:tools` | tools crate |
//! | `agent-prompt.log` | `aemeath:agent:prompt` | prompt crate |
//! | `agent-hook.log` | `aemeath:agent:hook` | hook crate |
//! | `agent-storage.log` | `aemeath:agent:storage` | storage 层 |
//! | `agent-project.log` | `aemeath:agent:project` | project 层 |
//! | `agent-policy.log` | `aemeath:agent:policy` | policy 层 |
//! | `agent-audit.log` | `aemeath:agent:audit` | audit 层 |
//!
//! ## 审计日志（静态方法直写）
//!
//! | 文件 | 数据 | 写入方法 |
//! |------|------|----------|
//! | `agent-provider.log` | 用户输入 + LLM 输入 + LLM 输出 | `log_input` / `log_user_input` / `log_output` |
//!
//! ## 不变
//!
//! | 文件 | 说明 |
//! |------|------|
//! | `panic.log` | panic_hook.rs 直写，不纳入 UnifiedLogger |

pub mod context;
pub mod format;
pub mod rotation;
#[cfg(test)]
pub mod target_guard;
pub mod unified_logger;

pub use context::{
    app_version, boot_ts, current_chat_id, current_model, current_provider, current_request_id,
    current_role, current_turn, session_id, set_app_version, set_boot_ts, set_current_chat_id,
    set_current_model, set_current_provider, set_current_request_id, set_current_role,
    set_current_turn, set_session_id,
};
pub use format::{format_text_line_with_turn, timestamp_local_rfc3339};
pub use rotation::{is_rotated_log_path, rotated_path, timestamp_rfc3339};
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

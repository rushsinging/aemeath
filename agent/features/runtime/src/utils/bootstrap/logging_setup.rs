use crate::utils::bootstrap::config_paths as paths;
use crate::LOG_TARGET;
use logging::{self, OutputMode, UnifiedLogger};
use share::config::LoggingConfig;

/// 设置全局 session ID（只能调用一次）。委托 `logging::set_session_id`。
pub fn set_session_id(id: String) {
    logging::set_session_id(id);
}

/// 设置当前 turn。委托 `logging::set_current_turn`。
pub fn set_current_turn(turn: usize) {
    logging::set_current_turn(turn);
}

/// 初始化日志子系统。
///
/// - 默认：用 `UnifiedLogger::init` 写到 13 个日志文件（按 target 前缀路由）。
/// - `AEMEATH_LOG_STDERR=1` 或 `AEMEATH_LOG_STDERR=true`：用 `UnifiedLogger` 输出到 stderr
///   （JSON Lines 格式，CLI `-q` 模式调试用）。
///
/// 日志级别由 `config.json` 的 `logging` 段控制；`RUST_LOG` 环境变量始终优先。
pub fn init_logging(logging_config: &LoggingConfig) {
    let output_mode = if use_stderr_log_target() {
        OutputMode::Stderr
    } else {
        OutputMode::File
    };
    let logs_dir = paths::global_logs_dir();

    if let Err(err) = UnifiedLogger::init(
        &logs_dir,
        logging_config.max_bytes,
        logging_config.max_backups,
        logging_config.to_level_filter(),
        output_mode,
    ) {
        eprintln!("failed to init unified logger: {err}");
        return;
    }
    log::info!(target: LOG_TARGET,
        "logging initialized: filter={} mode={:?} logs_dir={}",
        std::env::var("RUST_LOG").unwrap_or_else(|_| logging_config.to_filter_string()),
        output_mode,
        logs_dir.display()
    );

    // 注入 boot_ts 和 app_version 到全局上下文
    logging::set_boot_ts(logging::timestamp_local_rfc3339());
    logging::set_app_version(share::VERSION.to_string());
}

fn use_stderr_log_target() -> bool {
    std::env::var("AEMEATH_LOG_STDERR")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

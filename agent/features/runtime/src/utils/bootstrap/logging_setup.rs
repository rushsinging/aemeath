use crate::utils::bootstrap::config_paths as paths;
use logging::{self, UnifiedLogger};
use share::config::LoggingConfig;

/// 设置全局 session ID（只能调用一次）。委托 `logging::set_session_id`。
pub fn set_session_id(id: String) {
    logging::set_session_id(id);
}

/// 设置当前 turn。委托 `logging::set_current_turn`。
pub fn set_current_turn(turn: usize) {
    logging::set_current_turn(turn);
}

/// 初始化日志子系统（feature #79 路径 C）。
///
/// - 默认：用 `UnifiedLogger::init` 写到 `~/.agents/logs/{aemeath,tui,hook,input,output,tool}.log`。
/// - `AEMEATH_LOG_STDERR=1` 或 `AEMEATH_LOG_STDERR=true`：回退到 `env_logger` 写 stderr
///   （CLI 模式调试用，会破坏 TUI 渲染）。
///
/// 日志级别由 `config.json` 的 `logging` 段控制；`RUST_LOG` 环境变量始终优先。
pub fn init_logging(logging_config: &LoggingConfig) {
    let default_filter = logging_config.to_filter_string();
    let use_stderr = use_stderr_log_target();
    let logs_dir = paths::global_logs_dir();

    if use_stderr {
        // 调试模式：写 stderr（破坏 TUI 渲染，适合 --no-tui 调试）
        let mut builder = env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or(&default_filter),
        );
        builder.format(|buf, record| {
            use std::io::Write;
            let session = logging::session_id().unwrap_or("????????");
            let turn = logging::current_turn();
            let module = record.module_path().unwrap_or(record.target());
            let line = logging::format_text_line_with_turn(
                session,
                turn,
                record.level().as_str(),
                module,
                &record.args().to_string(),
            );
            writeln!(buf, "{}", line)
        });
        builder.init();
        log::info!(
            "logging initialized: filter={} target=stderr logs_dir={}",
            std::env::var("RUST_LOG").unwrap_or(default_filter),
            logs_dir.display()
        );
    } else {
        // 默认：UnifiedLogger 统一入口，6 个文件按 record.target() 路由
        if let Err(err) = UnifiedLogger::init(
            &logs_dir,
            logging_config.max_bytes,
            logging_config.max_backups,
            logging_config.role_logs_enabled,
            logging_config.to_level_filter(),
        ) {
            eprintln!("failed to init unified logger: {err}; falling back to stderr");
            env_logger::Builder::from_env(
                env_logger::Env::default().default_filter_or(&default_filter),
            )
            .init();
            return;
        }
        log::info!(
            "logging initialized: filter={} target=unified logs_dir={}",
            std::env::var("RUST_LOG").unwrap_or(default_filter),
            logs_dir.display()
        );
    }
}

fn use_stderr_log_target() -> bool {
    std::env::var("AEMEATH_LOG_STDERR")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

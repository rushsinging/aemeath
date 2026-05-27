use crate::utils::bootstrap::config_paths as paths;
use logging::{self, LogFile};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// 全局 session ID，供日志格式化器使用
static SESSION_ID: OnceLock<String> = OnceLock::new();
static CURRENT_TURN: AtomicUsize = AtomicUsize::new(0);

/// 设置全局 session ID（只能调用一次）
pub fn set_session_id(id: String) {
    let _ = SESSION_ID.set(id);
}

pub fn set_current_turn(turn: usize) {
    CURRENT_TURN.store(turn, Ordering::Relaxed);
}

fn current_turn_for_log() -> Option<usize> {
    match CURRENT_TURN.load(Ordering::Relaxed) {
        0 => None,
        turn => Some(turn),
    }
}

pub fn init_logging(logging_config: &crate::api::core::config::LoggingConfig) {
    // 初始化结构化日志 — 路由到 ~/.agents/logs/aemeath.log，避免库的 log::warn! / log::error! 破坏 TUI 渲染。
    // 设置 AEMEATH_LOG_STDERR=1 可在使用 --no-tui / CLI 模式调试时恢复 stderr 行为。
    // 日志级别由 config.json 的 logging 段控制；可通过 RUST_LOG 环境变量覆盖。
    let default_filter = logging_config.to_filter_string();
    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(&default_filter),
    );
    let use_stderr = std::env::var("AEMEATH_LOG_STDERR")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !use_stderr {
        if let Ok(file) = logging::open_append(&paths::global_logs_dir(), LogFile::Aemeath) {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
    }
    builder.format(|buf, record| {
        use std::io::Write;
        let session = SESSION_ID.get().map(|s| s.as_str()).unwrap_or("????????");
        writeln!(
            buf,
            "{}",
            logging::format_text_line_with_turn(
                session,
                current_turn_for_log(),
                record.level().as_str(),
                record.module_path().unwrap_or(record.target()),
                &record.args().to_string(),
            )
        )
    });
    builder.init();
}

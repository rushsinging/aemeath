use ::runtime::api::core::logging::{self, LogFile};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// 全局 session ID，供日志格式化器使用
static SESSION_ID: OnceLock<String> = OnceLock::new();
static CURRENT_TURN: AtomicUsize = AtomicUsize::new(0);

/// 设置全局 session ID（只能调用一次）
pub(crate) fn set_session_id(id: String) {
    let _ = SESSION_ID.set(id);
}

pub(crate) fn set_current_turn(turn: usize) {
    CURRENT_TURN.store(turn, Ordering::Relaxed);
}

fn current_turn_for_log() -> Option<usize> {
    match CURRENT_TURN.load(Ordering::Relaxed) {
        0 => None,
        turn => Some(turn),
    }
}

pub(crate) fn init_logging(logging_config: &::runtime::api::core::config::LoggingConfig) {
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
        if let Ok(file) = logging::open_append(LogFile::Aemeath) {
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

pub(crate) fn init_panic_hook() {
    std::panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());

        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let session = SESSION_ID.get().map(|s| s.as_str()).unwrap_or("????????");
        let backtrace = format!("{:?}", std::backtrace::Backtrace::capture());
        let msg = format!("{} at {}", payload, location);
        let extra = serde_json::json!({
            "location": location,
            "backtrace": backtrace,
        });

        let _ = logging::append_json_line_with_turn(
            LogFile::Panic,
            session,
            current_turn_for_log(),
            "ERROR",
            "panic",
            &msg,
            extra,
        );
        eprintln!("[PANIC] {}", msg);
    }));
}

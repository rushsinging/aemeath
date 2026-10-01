//! 进程级 panic hook — 自包含实现，不依赖 runtime。
//! 将 panic 信息写入 ~/.agents/logs/panic.log。
#![allow(dead_code)]

use super::constants::TERMINAL_RESTORE_SEQ;
use super::state::{CURRENT_RUN, SESSION_ID, TUI_ACTIVE};
use std::io::Write;
use std::sync::atomic::Ordering;

pub fn set_session_id(id: String) {
    let _ = SESSION_ID.set(id);
}

/// 进入/退出 TUI（raw + alternate screen）时调用，控制 panic 是否打印到 stderr。
pub fn set_tui_active(active: bool) {
    TUI_ACTIVE.store(active, Ordering::SeqCst);
}

pub fn set_current_run(run_step: usize) {
    CURRENT_RUN.store(run_step, std::sync::atomic::Ordering::Relaxed);
}

fn current_run_for_log() -> Option<usize> {
    match CURRENT_RUN.load(std::sync::atomic::Ordering::Relaxed) {
        0 => None,
        run_step => Some(run_step),
    }
}

/// panic hook 的终端恢复兜底：best-effort，忽略所有错误。
/// 覆盖 RAII guard 触达不到的场景（后台线程 panic、guard 被绕过）。
fn restore_terminal_best_effort() {
    let _ = crossterm::terminal::disable_raw_mode();
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(TERMINAL_RESTORE_SEQ);
    let _ = stdout.flush();
}

/// 从 panic payload 提取可读消息，供 panic hook、catch_unwind 兜底、后台 task 兜底复用。
pub fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string())
}

pub fn init_panic_hook() {
    std::panic::set_hook(Box::new(move |info| {
        let payload = payload_message(info.payload());

        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "unknown location".to_string());

        let session = SESSION_ID.get().map(|s| s.as_str()).unwrap_or("????????");
        let backtrace_str = format!("{:?}", std::backtrace::Backtrace::capture());

        let line = serde_json::json!({
            "session": session,
            "run_step": current_run_for_log(),
            "level": "ERROR",
            "module": "panic",
            "message": format!("{} at {}", payload, location),
            "payload": payload,
            "location": location,
            "backtrace": backtrace_str,
        });

        // 写入 ~/.agents/logs/panic.log
        if let Some(log_dir) = dirs::home_dir().map(|h| h.join(".agents").join("logs")) {
            let _ = std::fs::create_dir_all(&log_dir);
            let panic_log = log_dir.join("panic.log");
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&panic_log)
            {
                let _ = writeln!(file, "{}", line);
            }
        }

        // TUI 持有终端时，先恢复终端再打印——否则 stderr 会糊在 alternate screen 上。
        if TUI_ACTIVE.load(Ordering::SeqCst) {
            restore_terminal_best_effort();
            TUI_ACTIVE.store(false, Ordering::SeqCst);
        }
        eprintln!(
            "[PANIC] {} at {}（详见 ~/.agents/logs/panic.log）",
            payload, location
        );
    }));
}

#[cfg(test)]
#[path = "panic_hook_tests.rs"]
mod tests;

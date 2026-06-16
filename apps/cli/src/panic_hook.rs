//! 进程级 panic hook — 自包含实现，不依赖 runtime。
//! 将 panic 信息写入 ~/.agents/logs/panic.log。
#![allow(dead_code)]

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

static SESSION_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static CURRENT_TURN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// TUI 是否持有终端（raw mode + alternate screen）。为真时向 stderr 写 panic
/// 会糊到屏幕上，故此时只落 panic.log，不打印 stderr。
static TUI_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn set_session_id(id: String) {
    let _ = SESSION_ID.set(id);
}

/// 进入/退出 TUI（raw + alternate screen）时调用，控制 panic 是否打印到 stderr。
pub fn set_tui_active(active: bool) {
    TUI_ACTIVE.store(active, Ordering::SeqCst);
}

pub fn set_current_turn(turn: usize) {
    CURRENT_TURN.store(turn, std::sync::atomic::Ordering::Relaxed);
}

fn current_turn_for_log() -> Option<usize> {
    match CURRENT_TURN.load(std::sync::atomic::Ordering::Relaxed) {
        0 => None,
        turn => Some(turn),
    }
}

/// 终端恢复转义序列：LeaveAlternateScreen + DisableMouseCapture + DisableBracketedPaste + show cursor。
/// 与 TerminalGuard::drop 的恢复语义保持一致（此处为 panic hook 的最后兜底，不依赖 crossterm execute）。
const TERMINAL_RESTORE_SEQ: &[u8] = b"\x1b[?1049l\x1b[?1000l\x1b[?2004l\x1b[?25h";

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
            "turn": current_turn_for_log(),
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
mod tests {
    use super::*;

    #[test]
    fn test_payload_message_str() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("boom");
        assert_eq!(payload_message(payload.as_ref()), "boom");
    }

    #[test]
    fn test_payload_message_string() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("kaboom"));
        assert_eq!(payload_message(payload.as_ref()), "kaboom");
    }

    #[test]
    fn test_payload_message_unknown() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(42u32);
        assert_eq!(payload_message(payload.as_ref()), "unknown panic");
    }

    #[test]
    fn test_terminal_restore_seq_contains_leave_altscreen_and_show_cursor() {
        // \x1b[?1049l = LeaveAlternateScreen, \x1b[?25h = show cursor
        assert!(TERMINAL_RESTORE_SEQ
            .windows(8)
            .any(|w| w == b"\x1b[?1049l"));
        assert!(TERMINAL_RESTORE_SEQ.windows(6).any(|w| w == b"\x1b[?25h"));
    }
}

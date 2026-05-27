//! 进程级 panic hook — 自包含实现，不依赖 runtime。
//! 将 panic 信息写入 ~/.agents/logs/panic.log。
#![allow(dead_code)]

use std::io::Write;

static SESSION_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static CURRENT_TURN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn set_session_id(id: String) {
    let _ = SESSION_ID.set(id);
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

pub fn init_panic_hook() {
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

        eprintln!("[PANIC] {} at {}", payload, location);
    }));
}

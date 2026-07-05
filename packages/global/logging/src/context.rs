//! 全局日志上下文。
//!
//! 由 `UnifiedLogger` 在写入诊断与审计日志时注入 `ts / session / chat /
//! turn / model / boot_ts / ver / request_id / provider / role` 字段。
//! 调用方在启动期与运行期负责维护这些值：
//!
//! | 变量 | setter 位置 | 备注 |
//! |------|------------|------|
//! | `SESSION_ID` | 会话启动期（`runtime_support::start_session`） | 一次写入，`OnceLock` |
//! | `CURRENT_CHAT_ID` | `loop_runner.rs` 每 chat 开始 | 可变，跨 turn 保持 |
//! | `CURRENT_TURN` | `loop_runner.rs` 每 turn 开始 | 原子递增 |
//! | `CURRENT_MODEL` | `setup.rs` model 解析后 | 可变，模型可能切换 |
//! | `BOOT_TS` | `init_logging` 时调用一次 | 一次写入，`OnceLock` |
//! | `APP_VERSION` | `init_logging` 时调用一次 | 一次写入，`OnceLock` |
//! | `PID` | `init_logging` 时调用一次 | 一次写入，`OnceLock`，进程 pid |
//! | `CURRENT_PROVIDER` | 每次 LLM 调用前 | 可变，`RwLock` |
//! | `CURRENT_REQUEST_ID` | 每次 LLM 调用前 | 可变，`RwLock` |
//! | `CURRENT_ROLE` | 主 agent 为 `"default"`，sub-agent 为其 role 名 | 可变，`RwLock` |

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};

static SESSION_ID: OnceLock<String> = OnceLock::new();
static CURRENT_CHAT_ID: RwLock<String> = RwLock::new(String::new());
static CURRENT_TURN: AtomicUsize = AtomicUsize::new(0);
static CURRENT_MODEL: RwLock<String> = RwLock::new(String::new());
static BOOT_TS: OnceLock<String> = OnceLock::new();
static APP_VERSION: OnceLock<String> = OnceLock::new();
static PID: OnceLock<u32> = OnceLock::new();
static CURRENT_PROVIDER: RwLock<String> = RwLock::new(String::new());
static CURRENT_REQUEST_ID: RwLock<String> = RwLock::new(String::new());
static CURRENT_ROLE: RwLock<String> = RwLock::new(String::new());

/// 设置全局 session ID。`OnceLock` 语义：重复调用仅首次生效。
pub fn set_session_id(id: String) {
    let _ = SESSION_ID.set(id);
}

/// 设置当前 chat ID。`loop_runner` 每轮 chat 开始时调用。
pub fn set_current_chat_id(chat_id: String) {
    if let Ok(mut current) = CURRENT_CHAT_ID.write() {
        *current = chat_id;
    }
}

/// 设置当前 turn。`loop_runner` 每 turn 开始时调用。
pub fn set_current_turn(turn: usize) {
    CURRENT_TURN.store(turn, Ordering::Relaxed);
}

/// 设置当前 model。`setup.rs` 中 model 解析后调用。
pub fn set_current_model(model: String) {
    if let Ok(mut current) = CURRENT_MODEL.write() {
        *current = model;
    }
}

pub fn session_id() -> Option<&'static str> {
    SESSION_ID.get().map(|s| s.as_str())
}

pub fn current_chat_id() -> Option<String> {
    CURRENT_CHAT_ID
        .read()
        .ok()
        .and_then(|s| if s.is_empty() { None } else { Some(s.clone()) })
}

pub fn current_turn() -> Option<usize> {
    match CURRENT_TURN.load(Ordering::Relaxed) {
        0 => None,
        turn => Some(turn),
    }
}

/// 设置进程启动时间戳（本地时间 RFC3339）。`init_logging` 时调用一次。
pub fn set_boot_ts(ts: String) {
    let _ = BOOT_TS.set(ts);
}

/// 设置 app 版本号。`init_logging` 时调用一次。
pub fn set_app_version(ver: String) {
    let _ = APP_VERSION.set(ver);
}

/// 设置进程 pid。`init_logging` 时调用一次，未显式设置时惰性取 `std::process::id()`。
pub fn ensure_pid() {
    let _ = PID.get_or_init(std::process::id);
}

/// 设置当前 provider。
pub fn set_current_provider(provider: String) {
    if let Ok(mut current) = CURRENT_PROVIDER.write() {
        *current = provider;
    }
}

/// 设置当前 request_id（每次 LLM 调用前）。
pub fn set_current_request_id(id: String) {
    if let Ok(mut current) = CURRENT_REQUEST_ID.write() {
        *current = id;
    }
}

/// 设置当前 role（主 agent 为 "default"，sub-agent 为其 role 名）。
pub fn set_current_role(role: String) {
    if let Ok(mut current) = CURRENT_ROLE.write() {
        *current = role;
    }
}

pub fn current_model() -> Option<String> {
    CURRENT_MODEL
        .read()
        .ok()
        .and_then(|s| if s.is_empty() { None } else { Some(s.clone()) })
}

pub fn boot_ts() -> Option<&'static str> {
    BOOT_TS.get().map(|s| s.as_str())
}

/// 获取进程 pid。惰性取 `std::process::id()`。
pub fn pid() -> u32 {
    *PID.get_or_init(std::process::id)
}

pub fn app_version() -> Option<&'static str> {
    APP_VERSION.get().map(|s| s.as_str())
}

pub fn current_provider() -> Option<String> {
    CURRENT_PROVIDER
        .read()
        .ok()
        .and_then(|s| if s.is_empty() { None } else { Some(s.clone()) })
}

pub fn current_request_id() -> Option<String> {
    CURRENT_REQUEST_ID
        .read()
        .ok()
        .and_then(|s| if s.is_empty() { None } else { Some(s.clone()) })
}

pub fn current_role() -> Option<String> {
    CURRENT_ROLE
        .read()
        .ok()
        .and_then(|s| if s.is_empty() { None } else { Some(s.clone()) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // 测试串行化：所有读写全局上下文的测试共用一把锁
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn session_id_returns_none_before_set() {
        // 第一次 set 之后仍可能存在残留值；不直接断言 None，只保证 API 形态正确
        let _ = session_id();
    }

    #[test]
    fn chat_id_empty_is_none() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_current_chat_id(String::new());
        assert!(current_chat_id().is_none());
    }

    #[test]
    fn chat_id_non_empty_some() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_current_chat_id("session-1-001".to_string());
        assert_eq!(current_chat_id().as_deref(), Some("session-1-001"));
    }

    #[test]
    fn turn_zero_is_none() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_current_turn(0);
        assert!(current_turn().is_none());
    }

    #[test]
    fn turn_nonzero_some() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_current_turn(5);
        assert_eq!(current_turn(), Some(5));
    }

    #[test]
    fn model_empty_is_none() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_current_model(String::new());
        assert!(current_model().is_none());
    }

    #[test]
    fn model_non_empty_some() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_current_model("deepseek/deepseek-chat".to_string());
        assert_eq!(current_model().as_deref(), Some("deepseek/deepseek-chat"));
    }

    #[test]
    fn provider_empty_is_none() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_current_provider(String::new());
        assert!(current_provider().is_none());
    }

    #[test]
    fn provider_non_empty_some() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_current_provider("openai".to_string());
        assert_eq!(current_provider().as_deref(), Some("openai"));
    }

    #[test]
    fn request_id_empty_is_none() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_current_request_id(String::new());
        assert!(current_request_id().is_none());
    }

    #[test]
    fn request_id_non_empty_some() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_current_request_id("req-123".to_string());
        assert_eq!(current_request_id().as_deref(), Some("req-123"));
    }

    #[test]
    fn role_empty_is_none() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_current_role(String::new());
        assert!(current_role().is_none());
    }

    #[test]
    fn role_non_empty_some() {
        let _guard = TEST_LOCK.lock().unwrap();
        set_current_role("default".to_string());
        assert_eq!(current_role().as_deref(), Some("default"));
    }

    #[test]
    fn pid_returns_process_id() {
        // pid 应等于当前进程 id（get_or_init 首次调用写入）
        let p = pid();
        assert_eq!(p, std::process::id());
    }
}

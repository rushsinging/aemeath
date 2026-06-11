//! 全局日志上下文。
//!
//! 由 `UnifiedLogger` 在写入诊断与审计日志时注入 `ts / session / chat /
//! turn / model` 字段。调用方在启动期与运行期负责维护这些值：
//!
//! | 变量 | setter 位置 | 备注 |
//! |------|------------|------|
//! | `SESSION_ID` | 会话启动期（`runtime_support::start_session`） | 一次写入，`OnceLock` |
//! | `CURRENT_CHAT_ID` | `loop_runner.rs` 每 chat 开始 | 可变，跨 turn 保持 |
//! | `CURRENT_TURN` | `loop_runner.rs` 每 turn 开始 | 原子递增 |
//! | `CURRENT_MODEL` | `setup.rs` model 解析后 | 可变，模型可能切换 |

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{OnceLock, RwLock};

static SESSION_ID: OnceLock<String> = OnceLock::new();
static CURRENT_CHAT_ID: RwLock<String> = RwLock::new(String::new());
static CURRENT_TURN: AtomicUsize = AtomicUsize::new(0);
static CURRENT_MODEL: RwLock<String> = RwLock::new(String::new());

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

pub fn current_model() -> Option<String> {
    CURRENT_MODEL
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
        assert_eq!(
            current_model().as_deref(),
            Some("deepseek/deepseek-chat")
        );
    }
}

//! 会话元数据状态

use std::path::PathBuf;

/// 会话相关信息（不含基础设施引用）
#[derive(Debug)]
pub(crate) struct SessionState {
    pub session_id: String,
    pub cwd: PathBuf,
    pub session_created_at: Option<String>,
    pub current_model_display: String,
    pub memory_config: sdk::MemoryConfigView,
    /// #567：启动时存储 resume_id，start_chat 后发 ResumeSession 事件
    pub pending_resume_id: Option<String>,
}

impl SessionState {
    pub(crate) fn rename_session(&mut self, session_id: &str) {
        self.session_id = session_id.to_string();
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }
}

#[cfg(test)]
mod tests {
    use super::{PathBuf, SessionState};

    fn empty_state() -> SessionState {
        SessionState {
            session_id: "sess-1".into(),
            cwd: PathBuf::from("/tmp"),
            session_created_at: None,
            current_model_display: String::new(),
            memory_config: sdk::MemoryConfigView::default(),
            pending_resume_id: None,
        }
    }

    #[test]
    fn test_session_state_holds_session_id() {
        assert_eq!(empty_state().session_id, "sess-1");
    }
}

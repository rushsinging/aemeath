//! 会话元数据状态

use std::path::PathBuf;

/// 会话相关信息（不含基础设施引用）
#[derive(Debug)]
pub(crate) struct SessionState {
    pub session_id: String,
    pub cwd: PathBuf,
    pub session_created_at: Option<String>,
    pub cached_sessions: Vec<(String, String)>,
    pub current_model_display: String,
    pub memory_config: sdk::MemoryConfigView,
}

impl SessionState {
    pub(crate) fn cache_sessions(&mut self, sessions: Vec<(String, String)>) {
        self.cached_sessions = sessions;
    }

    pub(crate) fn rename_session(&mut self, session_id: &str) {
        self.session_id = session_id.to_string();
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn cached_sessions(&self) -> &[(String, String)] {
        &self.cached_sessions
    }
}

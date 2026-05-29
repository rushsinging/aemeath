//! 会话元数据状态

use std::path::PathBuf;

/// 会话相关信息（不含基础设施引用）
#[derive(Debug)]
pub(crate) struct SessionState {
    pub session_id: String,
    pub cwd: PathBuf,
    pub session_created_at: Option<String>,
    pub cached_sessions: Vec<(String, String)>,
    pub cached_models: Vec<sdk::ModelSummary>,
    pub current_model_display: String,
    pub memory_config: sdk::MemoryConfigView,
}

impl SessionState {
    pub(crate) fn cache_sessions(&mut self, sessions: Vec<(String, String)>) {
        self.cached_sessions = sessions;
    }

    pub(crate) fn cache_models(&mut self, models: Vec<sdk::ModelSummary>) {
        self.cached_models = models;
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

    pub(crate) fn cached_models(&self) -> &[sdk::ModelSummary] {
        &self.cached_models
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
            cached_sessions: vec![],
            cached_models: vec![],
            current_model_display: String::new(),
            memory_config: sdk::MemoryConfigView::default(),
        }
    }

    fn model() -> sdk::ModelSummary {
        sdk::ModelSummary {
            provider: "anthropic".into(),
            id: "claude-id".into(),
            name: "claude".into(),
            context_window: 200_000,
            max_tokens: 8_000,
        }
    }

    #[test]
    fn test_cached_models_default_empty() {
        assert!(empty_state().cached_models().is_empty());
    }

    #[test]
    fn test_cache_models_stores_entries() {
        let mut state = empty_state();
        let m = model();
        state.cache_models(vec![m.clone()]);
        assert_eq!(state.cached_models(), &[m]);
    }

    #[test]
    fn test_cache_models_replaces_rather_than_appends() {
        let mut state = empty_state();
        state.cache_models(vec![model()]);
        state.cache_models(vec![]);
        assert!(state.cached_models().is_empty());
    }
}

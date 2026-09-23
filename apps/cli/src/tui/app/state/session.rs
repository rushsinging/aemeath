//! 会话元数据状态

use std::path::PathBuf;

/// 会话相关信息（不含基础设施引用）
#[derive(Debug, Default)]
pub(crate) struct SessionState {
    pub session_id: String,
    pub cwd: PathBuf,
    pub session_created_at: Option<String>,
    pub current_model_display: String,
    pub memory_config: sdk::MemoryConfigView,
    /// 事件流回填的模型列表缓存：`None` 表示尚未收到 `ModelList` 事件，
    /// `Some(vec![])` 表示 runtime 确认当前没有配置任何模型。
    pub cached_models: Option<Vec<sdk::ModelSummary>>,
    /// 事件流回填的最近 session 列表（id, summary），供 /resume 补全消费。
    pub cached_sessions: Vec<(String, String)>,
    /// /model 对话框在等待 `ModelList` 事件回填后自动打开。
    pub model_selection_pending: bool,
}

impl SessionState {
    pub(crate) fn rename_session(&mut self, session_id: &str) {
        self.session_id = session_id.to_string();
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 回填模型列表缓存（`ModelList` 事件消费点调用）。
    pub(crate) fn cache_models(&mut self, models: Vec<sdk::ModelSummary>) {
        self.cached_models = Some(models);
    }

    /// 回填 /resume 补全用的 session 列表缓存（`SessionList` 事件消费点调用）。
    pub(crate) fn cache_sessions(&mut self, sessions: Vec<(String, String)>) {
        self.cached_sessions = sessions;
    }
}

#[cfg(test)]
mod tests {
    use super::{PathBuf, SessionState};

    fn empty_state() -> SessionState {
        SessionState {
            session_id: "sess-1".into(),
            cwd: PathBuf::from("/tmp"),
            current_model_display: String::new(),
            ..SessionState::default()
        }
    }

    #[test]
    fn test_session_state_holds_session_id() {
        assert_eq!(empty_state().session_id, "sess-1");
    }

    #[test]
    fn cache_models_distinguishes_unloaded_from_confirmed_empty() {
        let mut state = empty_state();
        assert!(
            state.cached_models.is_none(),
            "初始状态必须是未加载（None），区别于确认为空"
        );

        state.cache_models(Vec::new());

        assert_eq!(
            state.cached_models,
            Some(Vec::new()),
            "收到空 ModelList 后必须落为 Some(空)，表示 runtime 确认无模型"
        );
    }

    #[test]
    fn cache_models_replaces_previous_entries() {
        let mut state = empty_state();
        state.cache_models(vec![sdk::ModelSummary {
            provider: "anthropic".into(),
            id: "claude-3".into(),
            name: "Claude 3".into(),
            context_window: 200_000,
            max_tokens: 8_000,
        }]);
        state.cache_models(vec![sdk::ModelSummary {
            provider: "openai".into(),
            id: "gpt-5".into(),
            name: "GPT-5".into(),
            context_window: 400_000,
            max_tokens: 16_000,
        }]);

        let cached = state.cached_models.expect("缓存应已回填");
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].provider, "openai");
    }

    #[test]
    fn cache_sessions_replaces_previous_entries() {
        let mut state = empty_state();
        state.cache_sessions(vec![("s-1".into(), "first".into())]);
        state.cache_sessions(vec![("s-2".into(), "second".into())]);

        assert_eq!(
            state.cached_sessions,
            vec![("s-2".to_string(), "second".to_string())]
        );
    }
}

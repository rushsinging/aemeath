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
    /// /resume 补全数据是否已请求过（按需拉取，防重复触发 list 查询）。
    pub session_list_requested: bool,
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
        self.session_list_requested = true;
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

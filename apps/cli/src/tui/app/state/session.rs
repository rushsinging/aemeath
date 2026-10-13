//! 会话元数据状态

use std::path::PathBuf;

/// 会话相关信息（不含基础设施引用）
#[derive(Debug, Default)]
pub(crate) struct SessionState {
    pub session_id: String,
    pub cwd: PathBuf,
    pub session_created_at: Option<String>,
    pub current_model_display: String,
    /// 当前绑定模型的稳定身份 `(provider, id)`；显示名变更时靠它纠正 `current_model_display`。
    pub bound_model: Option<(String, String)>,
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

    /// 使模型列表缓存失效（回到未回填态），强制下次 `/model` 等待新 `ModelList`。
    pub(crate) fn invalidate_cached_models(&mut self) {
        self.cached_models = None;
    }

    /// 在清空模型缓存前，用当前显示名对照缓存解析并记住 `(provider, id)`。
    pub(crate) fn remember_bound_model_from_cache(&mut self) {
        if self.bound_model.is_some() {
            return;
        }
        let Some(models) = self.cached_models.as_deref() else {
            return;
        };
        if let Some(identity) = resolve_bound_model(&self.current_model_display, models) {
            self.bound_model = Some(identity);
        }
    }

    /// `ModelList` 回填后：记住身份，并把 `current_model_display` 纠正为最新 `provider/name`。
    /// 返回新的显示名（若有纠正或首次绑定成功）。
    pub(crate) fn sync_current_model_display_from_list(
        &mut self,
        models: &[sdk::ModelSummary],
    ) -> Option<String> {
        let identity = self
            .bound_model
            .clone()
            .or_else(|| resolve_bound_model(&self.current_model_display, models))?;
        self.bound_model = Some(identity.clone());
        let (provider, id) = identity;
        let model = models
            .iter()
            .find(|model| model.provider == provider && model.id == id)?;
        let label = if model.name.is_empty() {
            model.id.as_str()
        } else {
            model.name.as_str()
        };
        let display = format!("{provider}/{label}");
        if self.current_model_display == display {
            return None;
        }
        self.current_model_display = display.clone();
        Some(display)
    }

    pub(crate) fn clear_bound_model(&mut self) {
        self.bound_model = None;
    }

    /// 回填 /resume 补全用的 session 列表缓存（`SessionList` 事件消费点调用）。
    pub(crate) fn cache_sessions(&mut self, sessions: Vec<(String, String)>) {
        self.cached_sessions = sessions;
        self.session_list_requested = true;
    }
}

/// 用 `provider/name` 或 `provider/id` 显示串在列表中解析稳定身份。
fn resolve_bound_model(
    current_display: &str,
    models: &[sdk::ModelSummary],
) -> Option<(String, String)> {
    let (provider, rest) = current_display.split_once('/')?;
    if provider.is_empty() || rest.is_empty() {
        return None;
    }
    models.iter().find_map(|model| {
        if model.provider != provider {
            return None;
        }
        if model.id == rest || model.name == rest {
            Some((model.provider.clone(), model.id.clone()))
        } else {
            None
        }
    })
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;

//! ConfigSnapshot — immutable read-only view of merged configuration.
//!
//! Consumers obtain this via the `ConfigReader` port. They NEVER get
//! a mutable reference to `Config`. Field-level accessors expose only
//! what consumers need.

use std::sync::Arc;

use crate::config::models::{ModelEntryConfig, ModelResolveError, ModelsConfig, ResolvedModel};
use crate::config::permissions::PermissionModeConfig;
use crate::config::{
    AgentsConfig, Config, HooksConfig, LoggingConfig, MemoryConfig, ReasoningGraphConfig,
    SkillsConfig,
};

/// Immutable snapshot of effective configuration.
///
/// Wraps `Config` in `Arc` for cheap cloning via `watch::Receiver`.
/// All fields on the inner `Config` are accessed only through accessor
/// methods — consumers cannot mutate or reach the raw `Config`.
#[derive(Debug, Clone)]
pub struct ConfigSnapshot(Arc<Config>);

impl ConfigSnapshot {
    /// Create a new snapshot from a merged `Config`.
    pub fn new(config: Config) -> Self {
        Self(Arc::new(config))
    }

    /// Create a snapshot from an `Arc<Config>` (e.g. from `watch`).
    pub fn from_arc(config: Arc<Config>) -> Self {
        Self(config)
    }

    // ── API ──────────────────────────────────────────────────

    pub fn api_key(&self) -> Option<&str> {
        self.0.api.key.as_deref()
    }

    pub fn base_url(&self) -> Option<&str> {
        self.0.api.base_url.as_deref()
    }

    pub fn provider(&self) -> Option<&str> {
        self.0.api.provider.as_deref()
    }

    // ── Model ────────────────────────────────────────────────

    pub fn model_name(&self) -> &str {
        &self.0.model.name
    }

    pub fn max_tokens(&self) -> u32 {
        self.0.model.max_tokens
    }

    pub fn context_size(&self) -> usize {
        self.0.model.context_size
    }

    // ── Permissions ──────────────────────────────────────────

    pub fn permission_mode(&self) -> PermissionModeConfig {
        self.0.permissions.mode
    }

    pub fn allow_all(&self) -> bool {
        self.0.permissions.mode == PermissionModeConfig::AllowAll
    }

    // ── Tools / Agents ───────────────────────────────────────

    pub fn max_tool_concurrency(&self) -> usize {
        self.0.tools.max_concurrency
    }

    pub fn max_agent_concurrency(&self) -> usize {
        self.0.agents.max_concurrency
    }

    // ── Logging ──────────────────────────────────────────────

    pub fn logging_level(&self) -> &str {
        &self.0.logging.level
    }

    pub fn logs_dir(&self) -> Option<&str> {
        self.0.logging.logs_dir.as_deref()
    }

    // ── UI ───────────────────────────────────────────────────

    pub fn verbose(&self) -> bool {
        self.0.ui.verbose
    }

    pub fn color(&self) -> bool {
        self.0.ui.color
    }

    pub fn markdown(&self) -> bool {
        self.0.ui.markdown
    }

    pub fn tui(&self) -> bool {
        self.0.ui.tui
    }

    // ── Memory ───────────────────────────────────────────────

    pub fn memory_enabled(&self) -> bool {
        self.0.memory.enabled
    }

    // ── Storage ──────────────────────────────────────────────

    pub fn persist_sessions(&self) -> bool {
        self.0.storage.persist_sessions
    }

    // ── Guidance ─────────────────────────────────────────────

    pub fn language(&self) -> &str {
        &self.0.language
    }

    // ── Reasoning ────────────────────────────────────────────

    /// Resolve context size with CLI override.
    ///
    /// Priority: CLI explicit (non-zero) > snapshot (env > file already merged) >
    /// provider model context_window > default 128000.
    pub fn resolve_context_size(
        &self,
        cli_override: Option<usize>,
        model_context_window: usize,
    ) -> usize {
        // CLI explicit (non-zero) wins
        if let Some(cli) = cli_override {
            if cli > 0 {
                return cli;
            }
        }
        // snapshot value (already env > file merged)
        if self.0.model.context_size > 0 {
            return self.0.model.context_size;
        }
        // provider model contextWindow
        if model_context_window > 0 {
            return model_context_window;
        }
        // fallback default
        128_000
    }

    /// 返回完整 `ModelsConfig`，供消费方读取 providers / guidance / model entries 等。
    pub fn models(&self) -> &ModelsConfig {
        &self.0.models
    }

    /// 返回完整 `AgentsConfig`，供消费方读取 roles / max_concurrency 等。
    pub fn agents(&self) -> &AgentsConfig {
        &self.0.agents
    }

    /// 返回完整 `HooksConfig`，供 `build_hook_runner` 等消费。
    pub fn hooks(&self) -> &HooksConfig {
        &self.0.hooks
    }

    /// 返回完整 `MemoryConfig`，供 memory 命令 / 持久化逻辑消费。
    pub fn memory(&self) -> &MemoryConfig {
        &self.0.memory
    }

    /// 返回完整 `SkillsConfig`，供 `load_configured_skills` 消费。
    pub fn skills(&self) -> &SkillsConfig {
        &self.0.skills
    }

    /// 返回完整 `ReasoningGraphConfig`，供 `GraphRuntimeConfig::from_shared` 消费。
    pub fn reasoning_graph(&self) -> &ReasoningGraphConfig {
        &self.0.reasoning_graph
    }

    /// 返回完整 `LoggingConfig`，供 `init_logging` 消费。
    pub fn logging(&self) -> &LoggingConfig {
        &self.0.logging
    }

    /// 按 selection 字符串解析模型，委派给 `ModelsConfig::resolve_model_selection`。
    pub fn resolve_model_selection(
        &self,
        selection: &str,
    ) -> Result<ResolvedModel, ModelResolveError> {
        self.0.models.resolve_model_selection(selection)
    }

    /// 列出所有可用模型 `(source_key, ModelEntryConfig)`，委派给 `ModelsConfig::list_models`。
    pub fn list_models(&self) -> Vec<(String, ModelEntryConfig)> {
        self.0.models.list_models()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::models::ProviderModelsConfig;
    use crate::config::Config;

    #[test]
    fn test_resolve_context_size_cli_wins() {
        let mut config = Config::default();
        config.model.context_size = 32000;
        let snap = ConfigSnapshot::new(config);
        assert_eq!(snap.resolve_context_size(Some(64000), 0), 64000);
    }

    #[test]
    fn test_resolve_context_size_snapshot_wins() {
        let mut config = Config::default();
        config.model.context_size = 32000;
        let snap = ConfigSnapshot::new(config);
        assert_eq!(snap.resolve_context_size(None, 0), 32000);
    }

    #[test]
    fn test_resolve_context_size_model_window_fallback() {
        let config = Config::default();
        let snap = ConfigSnapshot::new(config);
        assert_eq!(snap.resolve_context_size(None, 96000), 96000);
    }

    #[test]
    fn test_resolve_context_size_default() {
        let config = Config::default();
        let snap = ConfigSnapshot::new(config);
        assert_eq!(snap.resolve_context_size(None, 0), 128_000);
    }

    #[test]
    fn test_resolve_context_size_cli_zero_ignored() {
        let mut config = Config::default();
        config.model.context_size = 32000;
        let snap = ConfigSnapshot::new(config);
        assert_eq!(snap.resolve_context_size(Some(0), 0), 32000);
    }

    #[test]
    fn test_substructure_accessors_return_config_fields() {
        let config = Config::default();
        let snap = ConfigSnapshot::new(config);
        // 子结构 accessor 应返回 snapshot 内部 Config 对应字段的引用
        assert_eq!(snap.models().default, Config::default().models.default);
        assert_eq!(
            snap.agents().max_concurrency,
            Config::default().agents.max_concurrency
        );
        assert_eq!(
            snap.hooks().events.len(),
            Config::default().hooks.events.len()
        );
        assert_eq!(snap.memory().enabled, Config::default().memory.enabled);
        assert_eq!(snap.skills().dirs, Config::default().skills.dirs);
        assert_eq!(
            snap.reasoning_graph().enabled,
            Config::default().reasoning_graph.enabled
        );
        assert_eq!(snap.logging().level, Config::default().logging.level);
    }

    #[test]
    fn test_resolve_model_selection_returns_resolved() {
        let mut config = Config::default();
        config.models.default = "zhipu/glm-5.1".to_string();
        config.models.providers.insert(
            "zhipu".to_string(),
            ProviderModelsConfig {
                driver: "zhipu".to_string(),
                models: vec![ModelEntryConfig {
                    id: "glm-5.1".to_string(),
                    name: "GLM 5.1".to_string(),
                    context_window: 128_000,
                    max_tokens: 4096,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let snap = ConfigSnapshot::new(config);
        let resolved = snap.resolve_model_selection("zhipu/glm-5.1");
        let resolved = resolved.expect("zhipu/glm-5.1 应解析成功");
        assert_eq!(resolved.source_key, "zhipu");
        assert_eq!(resolved.model.id, "glm-5.1");
        assert_eq!(resolved.driver, "zhipu");
    }

    #[test]
    fn test_resolve_model_selection_unknown_source_errors() {
        let config = Config::default();
        let snap = ConfigSnapshot::new(config);
        assert!(snap.resolve_model_selection("unknown/model").is_err());
    }

    #[test]
    fn test_list_models_returns_provider_entries() {
        let mut config = Config::default();
        config.models.providers.insert(
            "zhipu".to_string(),
            ProviderModelsConfig {
                driver: "zhipu".to_string(),
                models: vec![
                    ModelEntryConfig {
                        id: "glm-5.1".to_string(),
                        ..Default::default()
                    },
                    ModelEntryConfig {
                        id: "glm-5.2".to_string(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        );
        let snap = ConfigSnapshot::new(config);
        let entries = snap.list_models();
        assert_eq!(entries.len(), 2, "应返回两个 model entry");
        let ids: Vec<&str> = entries.iter().map(|(_, m)| m.id.as_str()).collect();
        assert!(ids.contains(&"glm-5.1"));
        assert!(ids.contains(&"glm-5.2"));
    }

    // ── PR-C: from_args snapshot accessor 组合测试 ──────────────────────
    //
    // 以下测试模拟 from_args.rs 中消费方拿到 ConfigSnapshot 后调 accessor
    // 的场景，验证非默认配置值能正确透传。

    /// Config.model.context_size=32000 时，消费方调 snapshot.context_size() 应得 32000。
    #[test]
    fn test_snapshot_context_size_priority() {
        // Arrange
        let mut config = Config::default();
        config.model.context_size = 32000;
        let snap = ConfigSnapshot::new(config);

        // Act & Assert
        assert_eq!(snap.context_size(), 32000);
    }

    /// Config.model.max_tokens=8192 时，消费方调 snapshot.max_tokens() 应得 8192。
    #[test]
    fn test_snapshot_max_tokens() {
        // Arrange
        let mut config = Config::default();
        config.model.max_tokens = 8192;
        let snap = ConfigSnapshot::new(config);

        // Act & Assert
        assert_eq!(snap.max_tokens(), 8192);
    }

    /// Config 含 tools.max_concurrency=8 / agents.max_concurrency=4 时，
    /// 消费方调对应 accessor 应得正确值。
    #[test]
    fn test_snapshot_concurrency_limits() {
        // Arrange
        let mut config = Config::default();
        config.tools.max_concurrency = 8;
        config.agents.max_concurrency = 4;
        let snap = ConfigSnapshot::new(config);

        // Act & Assert
        assert_eq!(snap.max_tool_concurrency(), 8);
        assert_eq!(snap.max_agent_concurrency(), 4);
    }

    /// resolve_context_size 在 CLI 传 0 时应忽略 CLI（用 snapshot 值），
    /// CLI 传 128000 时应直接使用 CLI 值。
    #[test]
    fn test_snapshot_resolve_context_size_with_model_window() {
        // Arrange — snapshot 值为 32000，model_window 为 96000
        let mut config = Config::default();
        config.model.context_size = 32000;
        let snap = ConfigSnapshot::new(config);

        // Act & Assert — CLI 0 被忽略，回退到 snapshot 32000
        assert_eq!(snap.resolve_context_size(Some(0), 96000), 32000);

        // Act & Assert — CLI 128000 覆盖 snapshot
        assert_eq!(snap.resolve_context_size(Some(128000), 96000), 128000);
    }

    /// Config 含 memory.enabled=true / reasoning_graph.enabled=true 时，
    /// 子结构 accessor 返回正确值。
    #[test]
    fn test_snapshot_memory_and_reasoning_graph() {
        // Arrange
        let mut config = Config::default();
        config.memory.enabled = true;
        config.reasoning_graph.enabled = true;
        let snap = ConfigSnapshot::new(config);

        // Act & Assert
        assert!(snap.memory().enabled, "memory().enabled 应为 true");
        assert!(
            snap.reasoning_graph().enabled,
            "reasoning_graph().enabled 应为 true"
        );
    }

    /// Config.language="zh" 时，snapshot.language() 应返回 "zh"。
    #[test]
    fn test_snapshot_language() {
        // Arrange
        let mut config = Config::default();
        config.language = "zh".to_string();
        let snap = ConfigSnapshot::new(config);

        // Act & Assert
        assert_eq!(snap.language(), "zh");
    }

    /// Default Config 的 language 应为 "en"。
    #[test]
    fn test_snapshot_language_default() {
        // Arrange
        let config = Config::default();
        let snap = ConfigSnapshot::new(config);

        // Act & Assert
        assert_eq!(snap.language(), "en");
    }
}

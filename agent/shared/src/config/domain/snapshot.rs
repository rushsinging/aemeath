//! ConfigSnapshot — immutable read-only view of merged configuration.
//!
//! Consumers obtain this via the `ConfigReader` port. They NEVER get
//! a mutable reference to `Config`. Field-level accessors expose only
//! what consumers need.

use std::sync::Arc;
use std::time::Duration;

use crate::config::audit::{DEFAULT_USAGE_QUEUE_CAPACITY, DEFAULT_USAGE_SHUTDOWN_TIMEOUT_MS};
use crate::config::models::{
    ModelEntryConfig, ModelResolveError, ModelsConfig, ResolvedModel, ResolvedRuntimeModel,
    RuntimeModelRequest, RuntimeModelResolutionError, RuntimeModelResolver,
};
use crate::config::permissions::PermissionModeConfig;
use crate::config::ui::{MarkdownSpacingMode, MarkdownSpacingOverrides};
use crate::config::{
    AgentsConfig, Config, HooksConfig, MemoryConfig, SkillsConfig, ToolResultConfig, ToolSelection,
};

const DEFAULT_HOOK_EXECUTION_MAX_ATTEMPTS: u8 = 3;
const DEFAULT_STOP_HOOK_MAX_BLOCKS: usize = 15;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct HookExecutionPolicy {
    max_attempts: u8,
}

impl HookExecutionPolicy {
    pub fn new(max_attempts: u8) -> Self {
        Self {
            max_attempts: if max_attempts > 0 {
                max_attempts
            } else {
                DEFAULT_HOOK_EXECUTION_MAX_ATTEMPTS
            },
        }
    }

    fn from_config(config: &HooksConfig) -> Self {
        Self::new(
            config
                .max_attempts
                .unwrap_or(DEFAULT_HOOK_EXECUTION_MAX_ATTEMPTS),
        )
    }

    pub fn max_attempts(self) -> u8 {
        self.max_attempts
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct StopHookPolicy {
    max_blocks: usize,
}

impl StopHookPolicy {
    pub fn new(max_blocks: usize) -> Self {
        Self {
            max_blocks: if max_blocks > 0 {
                max_blocks
            } else {
                DEFAULT_STOP_HOOK_MAX_BLOCKS
            },
        }
    }

    fn from_config(config: &HooksConfig) -> Self {
        Self::new(
            config
                .max_stop_hook_blocks
                .unwrap_or(DEFAULT_STOP_HOOK_MAX_BLOCKS),
        )
    }

    pub fn max_blocks(self) -> usize {
        self.max_blocks
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ToolResultPolicy {
    threshold_chars: usize,
    preview_head_chars: usize,
    preview_tail_chars: usize,
}

impl ToolResultPolicy {
    /// 截断阈值占 context window 的比例上限（1/20 = 5%）。
    ///
    /// 定量阈值只对大窗口合理：128k 窗口下单条 50k chars（中文场景约
    /// 50k tokens）即占 40%，会直接把启发式估算顶到 auto-compact 阈值。
    const WINDOW_SCALED_THRESHOLD_RATIO_DIVISOR: usize = 20;

    /// 窗口收紧后的阈值下限：过小的 preview 无法容纳有效的 head/tail 提示。
    const MIN_WINDOW_SCALED_THRESHOLD_CHARS: usize = 4_000;

    fn from_config(config: &ToolResultConfig) -> Self {
        let valid = config.threshold_chars > 0
            && config.preview_head_chars + config.preview_tail_chars <= config.threshold_chars;
        let config = if valid {
            config.clone()
        } else {
            ToolResultConfig::default()
        };
        Self {
            threshold_chars: config.threshold_chars,
            preview_head_chars: config.preview_head_chars,
            preview_tail_chars: config.preview_tail_chars,
        }
    }

    /// 按 context window 收紧截断阈值。
    ///
    /// - `threshold = min(配置值, 窗口 × 5%)`，下限 4k chars；配置值语义
    ///   是"大窗口下的上限"
    /// - head/tail 等比收紧到 threshold 的 1/4、1/8，收紧后仍满足
    ///   `head + tail ≤ threshold` 不变式（1/4 + 1/8 = 3/8 < 1）
    /// - `context_size == 0`（窗口未知）时不收紧，避免误伤大窗口
    pub fn scaled_for_context_window(self, context_size: usize) -> Self {
        if context_size == 0 {
            return self;
        }
        let ratio_cap = context_size / Self::WINDOW_SCALED_THRESHOLD_RATIO_DIVISOR;
        let threshold_chars = self
            .threshold_chars
            .min(ratio_cap.max(Self::MIN_WINDOW_SCALED_THRESHOLD_CHARS));
        Self {
            threshold_chars,
            preview_head_chars: self.preview_head_chars.min(threshold_chars / 4),
            preview_tail_chars: self.preview_tail_chars.min(threshold_chars / 8),
        }
    }

    pub fn threshold_chars(self) -> usize {
        self.threshold_chars
    }

    pub fn preview_head_chars(self) -> usize {
        self.preview_head_chars
    }

    pub fn preview_tail_chars(self) -> usize {
        self.preview_tail_chars
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct UsageWorkerConfig {
    capacity: usize,
    shutdown_timeout: Duration,
}

impl UsageWorkerConfig {
    pub fn capacity(self) -> usize {
        self.capacity
    }

    pub fn shutdown_timeout(self) -> Duration {
        self.shutdown_timeout
    }
}

/// Config-owned 单调版本号。每次 committed active state 切换恰好递增一次。
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ConfigRevision(u64);

impl ConfigRevision {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// Immutable snapshot of effective configuration.
///
/// Wraps `Config` in `Arc` for cheap cloning via `watch::Receiver`.
/// All fields on the inner `Config` are accessed only through accessor
/// methods — consumers cannot mutate or reach the raw `Config`.
#[derive(Debug, Clone)]
pub struct ConfigSnapshot {
    revision: ConfigRevision,
    inner: Arc<Config>,
}

impl ConfigSnapshot {
    /// Create a bootstrap snapshot. ConfigAppService commits use
    /// `new_with_revision` to preserve monotonic committed identity.
    pub fn new(config: Config) -> Self {
        Self::new_with_revision(ConfigRevision::default(), config)
    }

    pub fn new_with_revision(revision: ConfigRevision, config: Config) -> Self {
        Self {
            revision,
            inner: Arc::new(config),
        }
    }

    /// Create a new snapshot carrying the same config but a different revision.
    ///
    /// The `Arc<Config>` is shared (cheap clone); only the revision field changes.
    /// Used by `ConfigAppService` to stamp the next monotonic revision at commit time.
    pub fn with_revision(&self, revision: ConfigRevision) -> Self {
        Self {
            revision,
            inner: Arc::clone(&self.inner),
        }
    }

    /// Create a bootstrap snapshot from an `Arc<Config>` (e.g. from `watch`).
    pub fn from_arc(config: Arc<Config>) -> Self {
        Self {
            revision: ConfigRevision::default(),
            inner: config,
        }
    }

    pub fn revision(&self) -> ConfigRevision {
        self.revision
    }

    /// Config-owned application service uses this owned clone to install the
    /// exact prepared candidate as its committed active state.
    pub fn to_config(&self) -> Config {
        (*self.inner).clone()
    }

    // ── API ──────────────────────────────────────────────────

    pub fn api_key(&self) -> Option<&str> {
        self.inner.api.key.as_deref()
    }

    pub fn base_url(&self) -> Option<&str> {
        self.inner.api.base_url.as_deref()
    }

    pub fn provider(&self) -> Option<&str> {
        self.inner.api.provider.as_deref()
    }

    pub fn user_agent(&self) -> &str {
        &self.inner.api.user_agent
    }

    pub fn api_timeout_secs(&self) -> u64 {
        self.inner.api.timeout
    }

    // ── Model ────────────────────────────────────────────────

    pub fn model_name(&self) -> &str {
        &self.inner.model.name
    }

    pub fn max_tokens(&self) -> u32 {
        if self.inner.model.max_tokens > 0 {
            self.inner.model.max_tokens
        } else {
            crate::config::models::DEFAULT_MAX_TOKENS
        }
    }

    pub fn context_size(&self) -> usize {
        self.inner.model.context_size
    }

    // ── Context read reduction ────────────────────────────────

    pub fn context_snip_enabled(&self) -> bool {
        self.inner.context.snip_enabled
    }

    pub fn context_microcompact_enabled(&self) -> bool {
        self.inner.context.microcompact_enabled
    }

    pub fn auto_compact_failure_limit(&self) -> u8 {
        self.inner.context.auto_compact_failure_limit.max(1)
    }

    /// Compact 专用模型 selection；`None` 表示跟随当前会话模型。
    ///
    /// 再次归一化空白，避免绕过 `ContextConfig` 反序列化的程序化构造
    /// 让空白 selection 被误当成"已配置"。
    pub fn context_compact_model(&self) -> Option<&str> {
        self.inner
            .context
            .compact_model
            .as_deref()
            .map(str::trim)
            .filter(|selection| !selection.is_empty())
    }

    // ── Permissions ──────────────────────────────────────────

    pub fn permission_mode(&self) -> PermissionModeConfig {
        self.inner.permissions.mode
    }

    pub fn allow_all(&self) -> bool {
        self.inner.permissions.mode == PermissionModeConfig::AllowAll
    }

    // ── Tools / Agents ───────────────────────────────────────

    pub fn tool_selection(&self) -> ToolSelection {
        ToolSelection::new(&self.inner.tools.enabled, &self.inner.tools.disabled)
    }

    pub fn max_tool_concurrency(&self) -> usize {
        if self.inner.tools.max_concurrency > 0 {
            self.inner.tools.max_concurrency
        } else {
            super::tools::default_max_tool_concurrency()
        }
    }

    pub fn max_agent_concurrency(&self) -> usize {
        if self.inner.agents.max_concurrency > 0 {
            self.inner.agents.max_concurrency
        } else {
            super::tools::default_max_agent_concurrency()
        }
    }

    /// 构造 tool result 截断策略：先按配置校验归一，再按 `context_size`
    /// 比例收紧（见 [`ToolResultPolicy::scaled_for_context_window`]）。
    /// 调用方必须传入已解析的 context window；传 0 视为窗口未知，不收紧。
    pub fn tool_result_policy(&self, context_size: usize) -> ToolResultPolicy {
        ToolResultPolicy::from_config(&self.inner.tools.tool_result)
            .scaled_for_context_window(context_size)
    }

    // ── Logging ──────────────────────────────────────────────

    pub fn logging_level(&self) -> &str {
        &self.inner.logging.level
    }

    pub fn logs_dir(&self) -> Option<&str> {
        self.inner.logging.logs_dir.as_deref()
    }

    pub fn logging_max_bytes(&self) -> u64 {
        self.inner.logging.max_bytes
    }

    pub fn logging_max_backups(&self) -> usize {
        self.inner.logging.max_backups
    }

    pub fn logging_retention_days(&self) -> u64 {
        self.inner.logging.retention_days
    }

    // ── UI ───────────────────────────────────────────────────

    pub fn verbose(&self) -> bool {
        self.inner.ui.verbose
    }

    pub fn color(&self) -> bool {
        self.inner.ui.color
    }

    pub fn markdown(&self) -> bool {
        self.inner.ui.markdown
    }

    pub fn tui(&self) -> bool {
        self.inner.ui.tui
    }

    pub fn markdown_spacing_mode(&self) -> MarkdownSpacingMode {
        self.inner.ui.markdown_spacing
    }

    pub fn markdown_spacing_overrides(&self) -> MarkdownSpacingOverrides {
        self.inner.ui.markdown_spacing_overrides
    }

    // ── Memory ───────────────────────────────────────────────

    pub fn memory_enabled(&self) -> bool {
        self.inner.memory.enabled
    }

    // ── Audit ───────────────────────────────────────────────

    pub fn usage_worker_config(&self) -> UsageWorkerConfig {
        UsageWorkerConfig {
            capacity: if self.inner.audit.usage_queue_capacity > 0 {
                self.inner.audit.usage_queue_capacity
            } else {
                DEFAULT_USAGE_QUEUE_CAPACITY
            },
            shutdown_timeout: Duration::from_millis(
                if self.inner.audit.usage_shutdown_timeout_ms > 0 {
                    self.inner.audit.usage_shutdown_timeout_ms
                } else {
                    DEFAULT_USAGE_SHUTDOWN_TIMEOUT_MS
                },
            ),
        }
    }

    // ── Storage ──────────────────────────────────────────────

    pub fn persist_sessions(&self) -> bool {
        self.inner.storage.persist_sessions
    }

    // ── Guidance ─────────────────────────────────────────────

    pub fn guidance_reload_policy(&self) -> crate::config::GuidanceReloadPolicy {
        self.inner.guidance.reload_policy
    }

    pub fn language(&self) -> &str {
        &self.inner.language
    }

    // ── Reasoning ────────────────────────────────────────────

    /// Resolve context size with CLI override.
    ///
    /// Priority: CLI explicit (non-zero) > snapshot (env > file already merged) >
    /// provider model context_window > default 128000.
    ///
    /// When the snapshot value is adopted but is suspiciously smaller than the
    /// model registry window (see [`Self::context_size_mismatch_hint`]), a
    /// warning is logged — the configured value still wins (explicit user
    /// intent), but the mismatch stays observable (#1626).
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
        if self.inner.model.context_size > 0 {
            if let Some((configured, registry)) =
                self.context_size_mismatch_hint(model_context_window)
            {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "[config] 配置的 context_size {} 明显小于模型 registry 窗口 {}（低于 50%）——若非有意限制，请检查 aemeath.json 的 model.context_size / max_output_tokens 是否与模型真实窗口匹配，否则短窗口会频繁触发 auto-compact",
                    configured,
                    registry,
                );
            }
            return self.inner.model.context_size;
        }
        // provider model contextWindow
        if model_context_window > 0 {
            return model_context_window;
        }
        // fallback default
        128_000
    }

    /// 检测 snapshot `context_size` 与 model registry 窗口的疑似误配（#1626）。
    ///
    /// 返回 `Some((configured, registry))` 当且仅当：snapshot 显式配置了
    /// `context_size > 0`、registry 窗口已知，且配置值低于 registry 窗口的
    /// 50%。只提供提示信号，不改变 resolve 优先级。
    pub fn context_size_mismatch_hint(
        &self,
        model_context_window: usize,
    ) -> Option<(usize, usize)> {
        let configured = self.inner.model.context_size;
        (configured > 0 && model_context_window > 0 && configured * 2 < model_context_window)
            .then_some((configured, model_context_window))
    }

    /// 返回完整 `ModelsConfig`，供消费方读取 providers / guidance / model entries 等。
    pub fn models(&self) -> &ModelsConfig {
        &self.inner.models
    }

    /// 返回完整 `AgentsConfig`，供消费方读取 roles / max_concurrency 等。
    pub fn agents(&self) -> &AgentsConfig {
        &self.inner.agents
    }

    /// 返回完整 `HooksConfig`，供 subscription 转换消费。
    pub fn hooks(&self) -> &HooksConfig {
        &self.inner.hooks
    }

    pub fn hook_execution_policy(&self) -> HookExecutionPolicy {
        HookExecutionPolicy::from_config(&self.inner.hooks)
    }

    pub fn stop_hook_policy(&self) -> StopHookPolicy {
        StopHookPolicy::from_config(&self.inner.hooks)
    }

    /// 返回完整 `MemoryConfig`，供 memory 命令 / 持久化逻辑消费。
    pub fn memory(&self) -> &MemoryConfig {
        &self.inner.memory
    }

    /// 返回完整 `SkillsConfig`，供 `load_configured_skills` 消费。
    pub fn skills(&self) -> &SkillsConfig {
        &self.inner.skills
    }

    /// 按 selection 字符串解析模型，委派给 `ModelsConfig::resolve_model_selection`。
    pub fn resolve_model_selection(
        &self,
        selection: &str,
    ) -> Result<ResolvedModel, ModelResolveError> {
        self.inner.models.resolve_model_selection(selection)
    }

    /// 解析本次运行使用的模型与运行参数。
    pub fn resolve_runtime_model(
        &self,
        model_override: Option<&str>,
        cli_max_tokens: Option<u32>,
    ) -> Result<ResolvedRuntimeModel, RuntimeModelResolutionError> {
        RuntimeModelResolver::resolve(
            &self.inner.models,
            RuntimeModelRequest {
                model_override,
                cli_max_tokens,
                config_max_tokens: Some(self.inner.model.max_tokens),
            },
        )
    }

    /// 列出所有可用模型 `(source_key, ModelEntryConfig)`，委派给 `ModelsConfig::list_models`。
    pub fn list_models(&self) -> Vec<(String, ModelEntryConfig)> {
        self.inner.models.list_models()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::models::ProviderModelsConfig;
    use crate::config::Config;

    #[test]
    fn user_agent_accessor_returns_configured_value() {
        let mut config = Config::default();
        config.api.user_agent = "aemeath-test/1.0".to_string();

        assert_eq!(ConfigSnapshot::new(config).user_agent(), "aemeath-test/1.0");
    }

    #[test]
    fn logging_accessors_publish_complete_static_settings() {
        let mut config = Config::default();
        config.logging.level = "debug".to_string();
        config.logging.logs_dir = Some("custom/logs".to_string());
        config.logging.max_bytes = 42;
        config.logging.max_backups = 3;
        config.logging.retention_days = 14;
        let snapshot = ConfigSnapshot::new(config);

        assert_eq!(snapshot.logging_level(), "debug");
        assert_eq!(snapshot.logs_dir(), Some("custom/logs"));
        assert_eq!(snapshot.logging_max_bytes(), 42);
        assert_eq!(snapshot.logging_max_backups(), 3);
        assert_eq!(snapshot.logging_retention_days(), 14);
    }

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

    /// #1626：snapshot context_size 明显小于 model registry 真实窗口
    /// （< 50%）时视为疑似误配——返回值仍以 snapshot 为准（尊重显式配置），
    /// 但必须留下可观测提示（warn + 独立可测的 hint 方法）。
    #[test]
    fn test_context_size_mismatch_hint_flags_suspiciously_small_window() {
        let mut config = Config::default();
        config.model.context_size = 8192;
        let snap = ConfigSnapshot::new(config);

        // 8192 < 200_000 / 2 → 疑似误配，hint 返回 registry 窗口供提示
        assert_eq!(
            snap.context_size_mismatch_hint(200_000),
            Some((8192, 200_000))
        );
        // 接近真实窗口（8192 ≥ 16384/2，不低于 50%）不提示
        assert_eq!(snap.context_size_mismatch_hint(16_384), None);
        // 未配置 snapshot 值（0 = 未设置）不提示
        let unset = ConfigSnapshot::new(Config::default());
        assert_eq!(unset.context_size_mismatch_hint(200_000), None);
        // registry 窗口未知（0）不提示
        assert_eq!(snap.context_size_mismatch_hint(0), None);
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
        assert_eq!(snap.logging_level(), Config::default().logging.level);
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

    #[test]
    fn snapshot_auto_compact_failure_limit_defaults_and_normalizes_zero() {
        let default_snapshot = ConfigSnapshot::new(Config::default());
        assert_eq!(default_snapshot.auto_compact_failure_limit(), 3);

        let mut config = Config::default();
        config.context.auto_compact_failure_limit = 0;
        let normalized_snapshot = ConfigSnapshot::new(config);
        assert_eq!(normalized_snapshot.auto_compact_failure_limit(), 1);
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

    #[test]
    fn test_snapshot_max_tokens_zero_uses_default() {
        let mut config = Config::default();
        config.model.max_tokens = 0;
        let snap = ConfigSnapshot::new(config);

        assert_eq!(snap.max_tokens(), crate::config::models::DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn test_snapshot_resolve_runtime_model_model_wins_over_config() {
        let mut config = Config::default();
        config.model.max_tokens = 200_000;
        config.models.default = "zhipu/glm-5.1".to_string();
        config.models.providers.insert(
            "zhipu".to_string(),
            ProviderModelsConfig {
                driver: "zhipu".to_string(),
                models: vec![ModelEntryConfig {
                    id: "glm-5.1".to_string(),
                    name: "GLM 5.1".to_string(),
                    context_window: 128_000,
                    max_tokens: 8192,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let snap = ConfigSnapshot::new(config);

        let runtime_model = snap.resolve_runtime_model(None, None).unwrap();

        assert_eq!(runtime_model.max_tokens(), 8192);
        assert_eq!(
            runtime_model.max_tokens_source(),
            crate::config::models::MaxTokensSource::Model
        );
    }

    #[test]
    fn test_snapshot_resolve_runtime_model_cli_wins() {
        let mut config = Config::default();
        config.model.max_tokens = 200_000;
        config.models.default = "zhipu/glm-5.1".to_string();
        config.models.providers.insert(
            "zhipu".to_string(),
            ProviderModelsConfig {
                driver: "zhipu".to_string(),
                models: vec![ModelEntryConfig {
                    id: "glm-5.1".to_string(),
                    name: "GLM 5.1".to_string(),
                    context_window: 128_000,
                    max_tokens: 8192,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let snap = ConfigSnapshot::new(config);

        let runtime_model = snap.resolve_runtime_model(None, Some(4096)).unwrap();

        assert_eq!(runtime_model.max_tokens(), 4096);
        assert_eq!(
            runtime_model.max_tokens_source(),
            crate::config::models::MaxTokensSource::Cli
        );
    }

    #[test]
    fn test_snapshot_resolve_runtime_model_cli_zero_errors() {
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
                    max_tokens: 8192,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let snap = ConfigSnapshot::new(config);

        let err = snap.resolve_runtime_model(None, Some(0)).unwrap_err();

        assert_eq!(
            err,
            crate::config::models::RuntimeModelResolutionError::CliMaxTokensZero
        );
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

    #[test]
    fn snapshot_concurrency_limits_use_domain_defaults_for_default_config() {
        let snap = ConfigSnapshot::new(Config::default());

        assert_eq!(snap.max_tool_concurrency(), 10);
        assert_eq!(snap.max_agent_concurrency(), 4);
    }

    #[test]
    fn snapshot_concurrency_limits_normalize_zero_to_domain_defaults() {
        let mut config = Config::default();
        config.tools.max_concurrency = 0;
        config.agents.max_concurrency = 0;
        let snap = ConfigSnapshot::new(config);

        assert_eq!(snap.max_tool_concurrency(), 10);
        assert_eq!(snap.max_agent_concurrency(), 4);
    }

    #[test]
    fn snapshot_exposes_validated_tool_result_policy() {
        let mut config = Config::default();
        config.tools.tool_result.threshold_chars = 8_000;
        config.tools.tool_result.preview_head_chars = 1_000;
        config.tools.tool_result.preview_tail_chars = 250;
        let snap = ConfigSnapshot::new(config);

        let policy = snap.tool_result_policy(1_000_000);
        assert_eq!(policy.threshold_chars(), 8_000);
        assert_eq!(policy.preview_head_chars(), 1_000);
        assert_eq!(policy.preview_tail_chars(), 250);
    }

    #[test]
    fn snapshot_normalizes_invalid_tool_result_policy_to_compatible_defaults() {
        let mut config = Config::default();
        config.tools.tool_result.threshold_chars = 0;
        config.tools.tool_result.preview_head_chars = 9_000;
        config.tools.tool_result.preview_tail_chars = 9_000;
        let snap = ConfigSnapshot::new(config);

        let policy = snap.tool_result_policy(1_000_000);
        assert_eq!(policy.threshold_chars(), 50_000);
        assert_eq!(policy.preview_head_chars(), 2_000);
        assert_eq!(policy.preview_tail_chars(), 500);
    }

    /// tool_result 截断阈值必须随 context window 比例收紧：
    /// `threshold = min(配置值, 窗口×5%)`，下限 4k chars；
    /// head/tail 等比收紧（threshold 的 1/4、1/8）且收紧后仍满足
    /// `head + tail ≤ threshold` 不变式；窗口未知（0）时不收紧。
    /// 配置值语义是"大窗口下的上限"——1M 窗口下默认 50k 占 5% 合理，
    /// 128k 窗口下单条 50k chars（中文场景约 50k tokens）即占 40%，
    /// 会直接把启发式估算顶到 auto-compact 阈值。
    #[test]
    fn tool_result_policy_scales_threshold_with_context_window() {
        let snap = ConfigSnapshot::new(Config::default());

        // 1M 窗口：5% = 50k，与默认配置相等，不收紧
        let policy = snap.tool_result_policy(1_000_000);
        assert_eq!(policy.threshold_chars(), 50_000);
        assert_eq!(policy.preview_head_chars(), 2_000);
        assert_eq!(policy.preview_tail_chars(), 500);

        // 200k 窗口：5% = 10k 收紧；head/tail 低于等比上限，保持原值
        let policy = snap.tool_result_policy(200_000);
        assert_eq!(policy.threshold_chars(), 10_000);
        assert_eq!(policy.preview_head_chars(), 2_000);
        assert_eq!(policy.preview_tail_chars(), 500);

        // 128k 窗口：5% = 6.4k；head 收紧到 6400/4 = 1600
        let policy = snap.tool_result_policy(128_000);
        assert_eq!(policy.threshold_chars(), 6_400);
        assert_eq!(policy.preview_head_chars(), 1_600);
        assert_eq!(policy.preview_tail_chars(), 500);

        // 32k 窗口：5% = 1600 低于下限，取 4k；head = 4000/4 = 1000
        let policy = snap.tool_result_policy(32_000);
        assert_eq!(policy.threshold_chars(), 4_000);
        assert_eq!(policy.preview_head_chars(), 1_000);
        assert_eq!(policy.preview_tail_chars(), 500);

        // 窗口未知（0）：不收紧，避免误伤
        let policy = snap.tool_result_policy(0);
        assert_eq!(policy.threshold_chars(), 50_000);
        assert_eq!(policy.preview_head_chars(), 2_000);
        assert_eq!(policy.preview_tail_chars(), 500);
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

    /// Config 只暴露仍受支持的 memory 子结构。
    #[test]
    fn test_snapshot_memory_accessor() {
        let mut config = Config::default();
        config.memory.enabled = true;
        let snap = ConfigSnapshot::new(config);

        assert!(snap.memory().enabled, "memory().enabled 应为 true");
    }

    #[test]
    fn retired_reasoning_graph_section_is_ignored_by_config() {
        let config: Config = serde_json::from_value(serde_json::json!({
            "reasoning_graph": {
                "enabled": true,
                "max_reasoning": "high",
                "nodes": { "plan": { "effort": "low" } }
            }
        }))
        .expect("unknown retired section should remain backward-readable");
        let serialized = serde_json::to_value(config).expect("config serializes");
        assert!(serialized.get("reasoning_graph").is_none());
    }

    /// Config.language="zh" 时，snapshot.language() 应返回 "zh"。
    #[test]
    fn test_snapshot_language() {
        // Arrange
        let config = Config {
            language: "zh".to_string(),
            ..Config::default()
        };
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

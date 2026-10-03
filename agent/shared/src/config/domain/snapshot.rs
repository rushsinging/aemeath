//! ConfigSnapshot — immutable read-only view of merged configuration.
//!
//! Consumers obtain this via the `ConfigReader` port. They NEVER get
//! a mutable reference to `Config`. Field-level accessors expose only
//! what consumers need.

use std::path::Path;
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

use super::constants::{
    DEFAULT_HOOK_EXECUTION_MAX_ATTEMPTS, DEFAULT_STOP_HOOK_MAX_BLOCKS,
    MIN_WINDOW_SCALED_THRESHOLD_CHARS, WINDOW_SCALED_THRESHOLD_RATIO_DIVISOR,
};

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
        let ratio_cap = context_size / WINDOW_SCALED_THRESHOLD_RATIO_DIVISOR;
        let threshold_chars = self
            .threshold_chars
            .min(ratio_cap.max(MIN_WINDOW_SCALED_THRESHOLD_CHARS));
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

/// snapshot `context_size` 与模型 registry 真实窗口的疑似失配方向。
///
/// 只提供提示信号，不改变 resolve 优先级——显式配置始终胜出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSizeMisalignment {
    /// 配置明显小于 registry 窗口（< 50%）——短窗口会频繁触发 auto-compact（#1626）。
    ConfiguredTooSmall { configured: usize, registry: usize },
    /// 配置明显大于 registry 窗口（> 200%）——summary 预算等按窗口比例的
    /// 派生预算会随配置膨胀，超出模型真实承载（#1686）。
    ConfiguredTooLarge { configured: usize, registry: usize },
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

    /// Auto-compact 触发阈值比例（已归一化）。
    ///
    /// 读取时 clamp 到 `[0.5, 0.95]`，避免绕过 `ContextConfig`
    /// 反序列化的程序化构造让越界值触发 compact 风暴或缓冲归零。
    pub fn auto_compact_threshold_ratio(&self) -> f64 {
        crate::config::context::clamp_auto_compact_threshold_ratio(
            self.inner.context.auto_compact_threshold_ratio,
        )
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

    /// Configured root directory for EnterWorktree defaults.
    ///
    /// Returns the raw configured value: relative paths resolve against the
    /// workspace root at wiring time (config layer has no workspace knowledge).
    /// `None` means the default `<agents dir>/worktrees` applies.
    pub fn worktrees_dir(&self) -> Option<&Path> {
        self.inner.storage.worktrees_dir.as_deref()
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
            match self.context_size_mismatch_hint(model_context_window) {
                Some(ContextSizeMisalignment::ConfiguredTooSmall {
                    configured,
                    registry,
                }) => {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "[config] 配置的 context_size {} 明显小于模型 registry 窗口 {}（低于 50%）——若非有意限制，请检查 aemeath.json 的 model.context_size / max_output_tokens 是否与模型真实窗口匹配，否则短窗口会频繁触发 auto-compact",
                        configured,
                        registry,
                    );
                }
                Some(ContextSizeMisalignment::ConfiguredTooLarge {
                    configured,
                    registry,
                }) => {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "[config] 配置的 context_size {} 明显大于模型 registry 窗口 {}（超过 200%）——疑似按更大窗口模型残留的配置；compact summary 预算等按窗口比例的派生预算会随之膨胀，若非有意放大，请检查 aemeath.json 的 model.context_size 是否与模型真实窗口匹配",
                        configured,
                        registry,
                    );
                }
                None => {}
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
    /// 返回失配方向与两窗口值当且仅当：snapshot 显式配置了
    /// `context_size > 0`、registry 窗口已知，且配置值偏离 registry 窗口
    /// 超过 2 倍——过小（< 50%，#1626）或过大（> 200%，#1686）都视为疑似
    /// 误配。恰好 2 倍不提示。只提供提示信号，不改变 resolve 优先级。
    pub fn context_size_mismatch_hint(
        &self,
        model_context_window: usize,
    ) -> Option<ContextSizeMisalignment> {
        let configured = self.inner.model.context_size;
        if configured == 0 || model_context_window == 0 {
            return None;
        }
        if configured * 2 < model_context_window {
            return Some(ContextSizeMisalignment::ConfiguredTooSmall {
                configured,
                registry: model_context_window,
            });
        }
        if configured > model_context_window * 2 {
            return Some(ContextSizeMisalignment::ConfiguredTooLarge {
                configured,
                registry: model_context_window,
            });
        }
        None
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

    /// 返回完整 `ScoringConfig`，供 composition 装配评分服务消费。
    pub fn scoring(&self) -> &crate::config::scoring::ScoringConfig {
        &self.inner.scoring
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
#[path = "snapshot_tests.rs"]
mod tests;

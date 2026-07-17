//! Priority-chain merge strategy for config patches.
//!
//! Overlays sparse `ConfigPatch` layers onto a full `Config`, producing a
//! merged result.  Pure domain logic — no I/O.

use serde::Deserialize;
use serde_json::Value;

use crate::config::{
    hooks::HooksConfig,
    legacy::{ApiConfig, ModelConfig},
    logging::{LoggingConfig, SubAgentLogConfig},
    memory::{MemoryConfig, ReflectionConfig},
    models::{ModelsConfig, ProviderModelsConfig},
    permissions::{PermissionConfig, PermissionModeConfig},
    skills::SkillsConfig,
    storage::StorageConfig,
    tools::{AgentRoleConfig, AgentsConfig, ToolResultConfig, ToolsConfig},
    ui::{TaskLifecycleConfig, TaskListConfig, UiConfig},
    Config, GuidanceConfig, GuidanceReloadPolicy,
};
use std::{collections::HashMap, path::PathBuf};

// ---------------------------------------------------------------------------
// Patch structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConfigPatch {
    #[serde(default)]
    pub api: Option<ApiConfigPatch>,
    #[serde(default)]
    pub model: Option<ModelConfigPatch>,
    #[serde(default)]
    pub models: Option<ModelsConfigPatch>,
    #[serde(default)]
    pub tools: Option<ToolsConfigPatch>,
    #[serde(default)]
    pub agents: Option<AgentsConfigPatch>,
    #[serde(default)]
    pub ui: Option<UiConfigPatch>,
    #[serde(default)]
    pub permissions: Option<PermissionConfigPatch>,
    #[serde(default)]
    pub skills: Option<SkillsConfigPatch>,
    #[serde(default)]
    pub storage: Option<StorageConfigPatch>,
    #[serde(default)]
    pub hooks: Option<HooksConfig>,
    #[serde(default)]
    pub memory: Option<MemoryConfigPatch>,
    #[serde(default)]
    pub logging: Option<LoggingConfigPatch>,
    #[serde(default)]
    pub guidance: Option<GuidanceConfigPatch>,
}

impl ConfigPatch {
    /// Create a patch that only overrides the hooks section.
    pub fn with_hooks(hooks: HooksConfig) -> Self {
        Self {
            hooks: Some(hooks),
            ..Default::default()
        }
    }

    /// Returns true if no field is set (all None).
    pub fn is_empty(&self) -> bool {
        self.api.is_none()
            && self.model.is_none()
            && self.models.is_none()
            && self.tools.is_none()
            && self.agents.is_none()
            && self.ui.is_none()
            && self.permissions.is_none()
            && self.skills.is_none()
            && self.storage.is_none()
            && self.memory.is_none()
            && self.logging.is_none()
            && self.guidance.is_none()
            && self.hooks.is_none()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApiConfigPatch {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default)]
    pub retries: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelConfigPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub context_size: Option<usize>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_k: Option<u32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub stop_sequences: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModelsConfigPatch {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub providers: Option<HashMap<String, ProviderModelsConfig>>,
    #[serde(default)]
    pub guidance: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ToolsConfigPatch {
    #[serde(default)]
    pub enabled: Option<Vec<String>>,
    #[serde(default)]
    pub disabled: Option<Vec<String>>,
    #[serde(default)]
    pub settings: Option<HashMap<String, Value>>,
    #[serde(default, alias = "maxConcurrency")]
    pub max_concurrency: Option<usize>,
    #[serde(default)]
    pub tool_result: Option<ToolResultConfigPatch>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ToolResultConfigPatch {
    #[serde(default)]
    pub threshold_chars: Option<usize>,
    #[serde(default)]
    pub preview_head_chars: Option<usize>,
    #[serde(default)]
    pub preview_tail_chars: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentsConfigPatch {
    #[serde(default, alias = "maxConcurrency")]
    pub max_concurrency: Option<usize>,
    #[serde(default)]
    pub roles: Option<HashMap<String, AgentRoleConfig>>,
    #[serde(default, alias = "defaultModel")]
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UiConfigPatch {
    #[serde(default)]
    pub markdown: Option<bool>,
    #[serde(default)]
    pub syntax_highlight: Option<bool>,
    #[serde(default)]
    pub progress: Option<bool>,
    #[serde(default)]
    pub color: Option<bool>,
    #[serde(default)]
    pub verbose: Option<bool>,
    #[serde(default)]
    pub tui: Option<bool>,
    #[serde(default)]
    pub task_list: Option<TaskListConfigPatch>,
    #[serde(default)]
    pub task_lifecycle: Option<TaskLifecycleConfigPatch>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaskListConfigPatch {
    #[serde(default)]
    pub max_lines: Option<usize>,
    #[serde(default)]
    pub fold_hint_format: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaskLifecycleConfigPatch {
    #[serde(default)]
    pub auto_clear_completed_on_new_turn: Option<bool>,
    #[serde(default)]
    pub interrupt_prompt_enabled: Option<bool>,
    #[serde(default)]
    pub interrupt_default_action: Option<String>,
    #[serde(default)]
    pub stale_remind_after_turns: Option<usize>,
    #[serde(default)]
    pub stale_remind_repeat_interval: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PermissionConfigPatch {
    #[serde(default)]
    pub mode: Option<PermissionModeConfig>,
    #[serde(default)]
    pub auto_approve: Option<Vec<String>>,
    #[serde(default)]
    pub deny: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SkillsConfigPatch {
    #[serde(default)]
    pub dirs: Option<Vec<PathBuf>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StorageConfigPatch {
    #[serde(default)]
    pub sessions_dir: Option<PathBuf>,
    #[serde(default)]
    pub persist_sessions: Option<bool>,
    #[serde(default)]
    pub max_sessions: Option<usize>,
    #[serde(default)]
    pub history: Option<bool>,
    #[serde(default)]
    pub history_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MemoryConfigPatch {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub max_entries: Option<usize>,
    #[serde(default)]
    pub similarity_threshold: Option<f64>,
    #[serde(default)]
    pub reflection: Option<ReflectionConfigPatch>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReflectionConfigPatch {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub interval_turns: Option<usize>,
    #[serde(default)]
    pub auto_apply_suggestions: Option<bool>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LoggingConfigPatch {
    #[serde(default, alias = "default_level")]
    pub level: Option<String>,
    #[serde(default)]
    pub max_bytes: Option<u64>,
    #[serde(default)]
    pub max_backups: Option<usize>,
    #[serde(default)]
    pub retention_days: Option<u64>,
    #[serde(default)]
    pub sub_agent_log: Option<SubAgentLogConfigPatch>,
    #[serde(default)]
    pub logs_dir: Option<String>,
    #[serde(default)]
    pub role_logs_enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SubAgentLogConfigPatch {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub include_request_payload: Option<bool>,
    #[serde(default)]
    pub max_payload_bytes: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct GuidanceConfigPatch {
    #[serde(default)]
    pub reload_policy: Option<String>,
}

// ---------------------------------------------------------------------------
// Top-level apply
// ---------------------------------------------------------------------------

/// Apply a sparse config patch onto a full `Config`.
///
/// Only fields explicitly present in `patch` overwrite the corresponding
/// `base` values; missing fields keep the base values.
pub fn apply_patch(mut base: Config, patch: ConfigPatch) -> Config {
    if let Some(api) = patch.api {
        base.api = apply_api_patch(base.api, api);
    }
    if let Some(model) = patch.model {
        base.model = apply_model_patch(base.model, model);
    }
    if let Some(models) = patch.models {
        base.models = apply_models_patch(base.models, models);
    }
    if let Some(tools) = patch.tools {
        base.tools = apply_tools_patch(base.tools, tools);
    }
    if let Some(agents) = patch.agents {
        base.agents = apply_agents_patch(base.agents, agents);
    }
    if let Some(ui) = patch.ui {
        base.ui = apply_ui_patch(base.ui, ui);
    }
    if let Some(permissions) = patch.permissions {
        base.permissions = apply_permission_patch(base.permissions, permissions);
    }
    if let Some(skills) = patch.skills {
        base.skills = apply_skills_patch(base.skills, skills);
    }
    if let Some(storage) = patch.storage {
        base.storage = apply_storage_patch(base.storage, storage);
    }
    if let Some(hooks) = patch.hooks {
        base.hooks = merge_hooks(base.hooks, hooks);
    }
    if let Some(memory) = patch.memory {
        base.memory = apply_memory_patch(base.memory, memory);
    }
    if let Some(logging) = patch.logging {
        base.logging = apply_logging_patch(base.logging, logging);
    }
    if let Some(guidance) = patch.guidance {
        base.guidance = apply_guidance_patch(base.guidance, guidance);
    }
    base
}

// ---------------------------------------------------------------------------
// Section-level helpers
// ---------------------------------------------------------------------------

pub(crate) fn apply_api_patch(mut base: ApiConfig, patch: ApiConfigPatch) -> ApiConfig {
    if let Some(v) = patch.provider {
        base.provider = Some(v);
    }
    if let Some(v) = patch.key {
        base.key = Some(v);
    }
    if let Some(v) = patch.base_url {
        base.base_url = Some(v);
    }
    if let Some(v) = patch.user_agent {
        base.user_agent = v;
    }
    if let Some(v) = patch.timeout {
        base.timeout = v;
    }
    if let Some(v) = patch.retries {
        base.retries = v;
    }
    base
}

pub(crate) fn apply_model_patch(mut base: ModelConfig, patch: ModelConfigPatch) -> ModelConfig {
    if let Some(v) = patch.name {
        base.name = v;
    }
    if let Some(v) = patch.max_tokens {
        base.max_tokens = v;
    }
    if let Some(v) = patch.context_size {
        base.context_size = v;
    }
    if let Some(v) = patch.temperature {
        base.temperature = Some(v);
    }
    if let Some(v) = patch.top_k {
        base.top_k = Some(v);
    }
    if let Some(v) = patch.top_p {
        base.top_p = Some(v);
    }
    if let Some(v) = patch.stop_sequences {
        base.stop_sequences = v;
    }
    base
}

pub(crate) fn apply_models_patch(mut base: ModelsConfig, patch: ModelsConfigPatch) -> ModelsConfig {
    if let Some(v) = patch.mode {
        base.mode = v;
    }
    if let Some(v) = patch.default {
        base.default = v;
    }
    if let Some(providers) = patch.providers {
        for (k, v) in providers {
            base.providers.insert(k, v);
        }
    }
    if let Some(guidance) = patch.guidance {
        for (k, v) in guidance {
            base.guidance.insert(k, v);
        }
    }
    base
}

pub(crate) fn apply_tools_patch(mut base: ToolsConfig, patch: ToolsConfigPatch) -> ToolsConfig {
    if let Some(v) = patch.enabled {
        base.enabled = v;
    }
    if let Some(v) = patch.disabled {
        base.disabled = v;
    }
    if let Some(settings) = patch.settings {
        for (k, v) in settings {
            base.settings.insert(k, v);
        }
    }
    if let Some(v) = patch.max_concurrency {
        base.max_concurrency = v;
    }
    if let Some(v) = patch.tool_result {
        base.tool_result = apply_tool_result_patch(base.tool_result, v);
    }
    base
}

fn apply_tool_result_patch(
    mut base: ToolResultConfig,
    patch: ToolResultConfigPatch,
) -> ToolResultConfig {
    if let Some(v) = patch.threshold_chars {
        base.threshold_chars = v;
    }
    if let Some(v) = patch.preview_head_chars {
        base.preview_head_chars = v;
    }
    if let Some(v) = patch.preview_tail_chars {
        base.preview_tail_chars = v;
    }
    base
}

pub(crate) fn apply_agents_patch(mut base: AgentsConfig, patch: AgentsConfigPatch) -> AgentsConfig {
    if let Some(v) = patch.max_concurrency {
        base.max_concurrency = v;
    }
    if let Some(roles) = patch.roles {
        for (k, v) in roles {
            base.roles.insert(k, v);
        }
    }
    if let Some(v) = patch.default_model {
        base.default_model = v;
    }
    base
}

pub(crate) fn apply_ui_patch(mut base: UiConfig, patch: UiConfigPatch) -> UiConfig {
    if let Some(v) = patch.markdown {
        base.markdown = v;
    }
    if let Some(v) = patch.syntax_highlight {
        base.syntax_highlight = v;
    }
    if let Some(v) = patch.progress {
        base.progress = v;
    }
    if let Some(v) = patch.color {
        base.color = v;
    }
    if let Some(v) = patch.verbose {
        base.verbose = v;
    }
    if let Some(v) = patch.tui {
        base.tui = v;
    }
    if let Some(v) = patch.task_list {
        base.task_list = apply_task_list_patch(base.task_list, v);
    }
    if let Some(v) = patch.task_lifecycle {
        base.task_lifecycle = apply_task_lifecycle_patch(base.task_lifecycle, v);
    }
    base
}

pub(crate) fn apply_task_list_patch(
    mut base: TaskListConfig,
    patch: TaskListConfigPatch,
) -> TaskListConfig {
    if let Some(v) = patch.max_lines {
        base.max_lines = v;
    }
    if let Some(v) = patch.fold_hint_format {
        base.fold_hint_format = v;
    }
    base
}

pub(crate) fn apply_task_lifecycle_patch(
    mut base: TaskLifecycleConfig,
    patch: TaskLifecycleConfigPatch,
) -> TaskLifecycleConfig {
    if let Some(v) = patch.auto_clear_completed_on_new_turn {
        base.auto_clear_completed_on_new_turn = v;
    }
    if let Some(v) = patch.interrupt_prompt_enabled {
        base.interrupt_prompt_enabled = v;
    }
    if let Some(v) = patch.interrupt_default_action {
        base.interrupt_default_action = v;
    }
    if let Some(v) = patch.stale_remind_after_turns {
        base.stale_remind_after_turns = v;
    }
    if let Some(v) = patch.stale_remind_repeat_interval {
        base.stale_remind_repeat_interval = v;
    }
    base
}

pub(crate) fn apply_permission_patch(
    mut base: PermissionConfig,
    patch: PermissionConfigPatch,
) -> PermissionConfig {
    if let Some(v) = patch.mode {
        base.mode = v;
    }
    if let Some(v) = patch.auto_approve {
        base.auto_approve = v;
    }
    if let Some(v) = patch.deny {
        base.deny = v;
    }
    base
}

pub(crate) fn apply_skills_patch(mut base: SkillsConfig, patch: SkillsConfigPatch) -> SkillsConfig {
    if let Some(v) = patch.dirs {
        base.dirs = v;
    }
    base
}

pub(crate) fn apply_storage_patch(
    mut base: StorageConfig,
    patch: StorageConfigPatch,
) -> StorageConfig {
    if let Some(v) = patch.sessions_dir {
        base.sessions_dir = Some(v);
    }
    if let Some(v) = patch.persist_sessions {
        base.persist_sessions = v;
    }
    if let Some(v) = patch.max_sessions {
        base.max_sessions = v;
    }
    if let Some(v) = patch.history {
        base.history = v;
    }
    if let Some(v) = patch.history_file {
        base.history_file = Some(v);
    }
    base
}

pub(crate) fn merge_hooks(base: HooksConfig, overlay: HooksConfig) -> HooksConfig {
    let mut events = base.events;
    for (k, v) in overlay.events {
        events.insert(k, v);
    }
    HooksConfig { events }
}

pub(crate) fn apply_memory_patch(mut base: MemoryConfig, patch: MemoryConfigPatch) -> MemoryConfig {
    if let Some(v) = patch.enabled {
        base.enabled = v;
    }
    if let Some(v) = patch.max_entries {
        base.max_entries = v;
    }
    if let Some(v) = patch.similarity_threshold {
        base.similarity_threshold = v;
    }
    if let Some(v) = patch.reflection {
        base.reflection = apply_reflection_patch(base.reflection, v);
    }
    base
}

pub(crate) fn apply_reflection_patch(
    mut base: ReflectionConfig,
    patch: ReflectionConfigPatch,
) -> ReflectionConfig {
    if let Some(v) = patch.enabled {
        base.enabled = v;
    }
    if let Some(v) = patch.interval_turns {
        base.interval_turns = v;
    }
    if let Some(v) = patch.auto_apply_suggestions {
        base.auto_apply_suggestions = v;
    }
    if let Some(v) = patch.model {
        base.model = Some(v);
    }
    base
}

pub(crate) fn apply_logging_patch(
    mut base: LoggingConfig,
    patch: LoggingConfigPatch,
) -> LoggingConfig {
    if let Some(v) = patch.level {
        base.level = v;
    }
    if let Some(v) = patch.max_bytes {
        base.max_bytes = v;
    }
    if let Some(v) = patch.max_backups {
        base.max_backups = v;
    }
    if let Some(v) = patch.retention_days {
        base.retention_days = v;
    }
    if let Some(v) = patch.sub_agent_log {
        base.sub_agent_log = apply_sub_agent_log_patch(base.sub_agent_log, v);
    }
    if let Some(v) = patch.logs_dir {
        base.logs_dir = Some(v);
    }
    if let Some(v) = patch.role_logs_enabled {
        base.role_logs_enabled = v;
    }
    base
}

pub(crate) fn apply_sub_agent_log_patch(
    mut base: SubAgentLogConfig,
    patch: SubAgentLogConfigPatch,
) -> SubAgentLogConfig {
    if let Some(v) = patch.enabled {
        base.enabled = v;
    }
    if let Some(v) = patch.include_request_payload {
        base.include_request_payload = v;
    }
    if let Some(v) = patch.max_payload_bytes {
        base.max_payload_bytes = v;
    }
    base
}

pub(crate) fn apply_guidance_patch(
    mut base: GuidanceConfig,
    patch: GuidanceConfigPatch,
) -> GuidanceConfig {
    if let Some(v) = patch.reload_policy {
        base.reload_policy = match v.as_str() {
            "inject" => GuidanceReloadPolicy::Inject,
            "remind" => GuidanceReloadPolicy::Remind,
            "confirm" => GuidanceReloadPolicy::Confirm,
            _ => {
                log::warn!(
                    target: "aemeath:shared",
                    "[config] unknown guidance.reload_policy '{}', keeping default",
                    v
                );
                base.reload_policy
            }
        };
    }
    base
}

// ---------------------------------------------------------------------------
// PriorityChain — ordered merge of multiple patches
// ---------------------------------------------------------------------------

/// An ordered list of [`ConfigPatch`]es that are merged in sequence.
///
/// Patches pushed earlier have **lower** priority; patches pushed later
/// overwrite earlier values.  Call [`merge`](Self::merge) to produce a
/// final [`Config`].
#[derive(Debug, Clone, Default)]
pub struct PriorityChain {
    patches: Vec<ConfigPatch>,
}

impl PriorityChain {
    /// Create an empty chain.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a patch layer (later pushes = higher priority).
    pub fn push(&mut self, patch: ConfigPatch) {
        self.patches.push(patch);
    }

    /// Consume the chain and merge all patches onto `base`, returning
    /// the final [`Config`].
    pub fn merge(self, base: Config) -> Config {
        self.patches.into_iter().fold(base, apply_patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::domain::snapshot::ConfigSnapshot;

    #[test]
    fn test_config_patch_snake_case_concurrency_reaches_snapshot() {
        let patch: ConfigPatch = serde_json::from_str(
            r#"{
                "tools": { "max_concurrency": 9 },
                "agents": {
                    "max_concurrency": 6,
                    "default_model": "snake/model",
                    "roles": { "coder": { "system_suffix": "snake" } }
                }
            }"#,
        )
        .unwrap();

        let snapshot = ConfigSnapshot::new(apply_patch(Config::default(), patch));

        assert_eq!(snapshot.max_tool_concurrency(), 9);
        assert_eq!(snapshot.max_agent_concurrency(), 6);
        assert_eq!(snapshot.agents().default_model, "snake/model");
        assert_eq!(
            snapshot.agents().roles["coder"].system_suffix.as_deref(),
            Some("snake")
        );
    }

    #[test]
    fn test_config_patch_accepts_legacy_agent_and_tool_aliases() {
        let patch: ConfigPatch = serde_json::from_str(
            r#"{
                "tools": { "maxConcurrency": 8 },
                "agents": { "maxConcurrency": 5, "defaultModel": "legacy/model" }
            }"#,
        )
        .unwrap();

        let snapshot = ConfigSnapshot::new(apply_patch(Config::default(), patch));

        assert_eq!(snapshot.max_tool_concurrency(), 8);
        assert_eq!(snapshot.max_agent_concurrency(), 5);
        assert_eq!(snapshot.agents().default_model, "legacy/model");
    }

    #[test]
    fn tool_result_partial_patch_preserves_unspecified_policy_fields() {
        let patch: ConfigPatch = serde_json::from_str(
            r#"{
                "tools": {
                    "tool_result": { "threshold_chars": 9000 }
                }
            }"#,
        )
        .unwrap();

        let snapshot = ConfigSnapshot::new(apply_patch(Config::default(), patch));
        let policy = snapshot.tool_result_policy();

        assert_eq!(policy.threshold_chars(), 9_000);
        assert_eq!(policy.preview_head_chars(), 2_000);
        assert_eq!(policy.preview_tail_chars(), 500);
    }
}

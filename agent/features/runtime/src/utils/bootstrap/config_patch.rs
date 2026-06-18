//! Config patch definitions and merge logic — overlays sparse config patches onto full configs.

use serde::Deserialize;
use serde_json::Value;
use share::config::{
    hooks::HooksConfig,
    legacy::{ApiConfig, ModelConfig},
    logging::{LoggingConfig, SubAgentLogConfig},
    memory::{MemoryConfig, ReflectionConfig},
    models::{ModelsConfig, ProviderModelsConfig},
    permissions::{PermissionConfig, PermissionModeConfig},
    skills::SkillsConfig,
    storage::StorageConfig,
    tools::{AgentRoleConfig, AgentsConfig, ToolsConfig},
    ui::{TaskLifecycleConfig, TaskListConfig, UiConfig},
    Config, GuidanceConfig,
};
use std::{collections::HashMap, path::PathBuf};

use super::config_manager::ConfigManager;
use crate::LOG_TARGET;

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ConfigPatch {
    #[serde(default)]
    api: Option<ApiConfigPatch>,
    #[serde(default)]
    model: Option<ModelConfigPatch>,
    #[serde(default)]
    models: Option<ModelsConfigPatch>,
    #[serde(default)]
    tools: Option<ToolsConfigPatch>,
    #[serde(default)]
    agents: Option<AgentsConfigPatch>,
    #[serde(default)]
    ui: Option<UiConfigPatch>,
    #[serde(default)]
    permissions: Option<PermissionConfigPatch>,
    #[serde(default)]
    skills: Option<SkillsConfigPatch>,
    #[serde(default)]
    storage: Option<StorageConfigPatch>,
    #[serde(default)]
    hooks: Option<HooksConfig>,
    #[serde(default)]
    memory: Option<MemoryConfigPatch>,
    #[serde(default)]
    logging: Option<LoggingConfigPatch>,
    #[serde(default)]
    guidance: Option<GuidanceConfigPatch>,
}

impl ConfigPatch {
    /// Create a patch that only overrides the hooks section.
    pub(crate) fn with_hooks(hooks: HooksConfig) -> Self {
        Self {
            hooks: Some(hooks),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ApiConfigPatch {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    user_agent: Option<String>,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    retries: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ModelConfigPatch {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    context_size: Option<usize>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    top_k: Option<u32>,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default)]
    stop_sequences: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ModelsConfigPatch {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    providers: Option<HashMap<String, ProviderModelsConfig>>,
    #[serde(default)]
    guidance: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ToolsConfigPatch {
    #[serde(default)]
    enabled: Option<Vec<String>>,
    #[serde(default)]
    disabled: Option<Vec<String>>,
    #[serde(default)]
    settings: Option<HashMap<String, Value>>,
    #[serde(default)]
    max_concurrency: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AgentsConfigPatch {
    #[serde(default)]
    max_concurrency: Option<usize>,
    #[serde(default)]
    roles: Option<HashMap<String, AgentRoleConfig>>,
    #[serde(default)]
    default_model: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct UiConfigPatch {
    #[serde(default)]
    markdown: Option<bool>,
    #[serde(default)]
    syntax_highlight: Option<bool>,
    #[serde(default)]
    progress: Option<bool>,
    #[serde(default)]
    color: Option<bool>,
    #[serde(default)]
    verbose: Option<bool>,
    #[serde(default)]
    tui: Option<bool>,
    #[serde(default)]
    task_list: Option<TaskListConfigPatch>,
    #[serde(default)]
    task_lifecycle: Option<TaskLifecycleConfigPatch>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TaskListConfigPatch {
    #[serde(default)]
    max_lines: Option<usize>,
    #[serde(default)]
    fold_hint_format: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TaskLifecycleConfigPatch {
    #[serde(default)]
    auto_clear_completed_on_new_turn: Option<bool>,
    #[serde(default)]
    interrupt_prompt_enabled: Option<bool>,
    #[serde(default)]
    interrupt_default_action: Option<String>,
    #[serde(default)]
    stale_remind_after_turns: Option<usize>,
    #[serde(default)]
    stale_remind_repeat_interval: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PermissionConfigPatch {
    #[serde(default)]
    mode: Option<PermissionModeConfig>,
    #[serde(default)]
    auto_approve: Option<Vec<String>>,
    #[serde(default)]
    deny: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SkillsConfigPatch {
    #[serde(default)]
    dirs: Option<Vec<PathBuf>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct StorageConfigPatch {
    #[serde(default)]
    sessions_dir: Option<PathBuf>,
    #[serde(default)]
    persist_sessions: Option<bool>,
    #[serde(default)]
    max_sessions: Option<usize>,
    #[serde(default)]
    history: Option<bool>,
    #[serde(default)]
    history_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MemoryConfigPatch {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    max_entries: Option<usize>,
    #[serde(default)]
    similarity_threshold: Option<f64>,
    #[serde(default)]
    reflection: Option<ReflectionConfigPatch>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ReflectionConfigPatch {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    interval_turns: Option<usize>,
    #[serde(default)]
    auto_apply_suggestions: Option<bool>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct LoggingConfigPatch {
    #[serde(default, alias = "default_level")]
    level: Option<String>,
    #[serde(default)]
    max_bytes: Option<u64>,
    #[serde(default)]
    max_backups: Option<usize>,
    #[serde(default)]
    retention_days: Option<u64>,
    #[serde(default)]
    sub_agent_log: Option<SubAgentLogConfigPatch>,
    #[serde(default)]
    logs_dir: Option<String>,
    #[serde(default)]
    role_logs_enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SubAgentLogConfigPatch {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    include_request_payload: Option<bool>,
    #[serde(default)]
    max_payload_bytes: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct GuidanceConfigPatch {
    #[serde(default)]
    reload_policy: Option<String>,
}

impl ConfigManager {
    /// Apply a sparse config patch (overlay takes precedence only for explicitly present fields).
    pub(crate) fn apply_patch(mut base: Config, patch: ConfigPatch) -> Config {
        if let Some(api) = patch.api {
            base.api = Self::apply_api_patch(base.api, api);
        }
        if let Some(model) = patch.model {
            base.model = Self::apply_model_patch(base.model, model);
        }
        if let Some(models) = patch.models {
            base.models = Self::apply_models_patch(base.models, models);
        }
        if let Some(tools) = patch.tools {
            base.tools = Self::apply_tools_patch(base.tools, tools);
        }
        if let Some(agents) = patch.agents {
            base.agents = Self::apply_agents_patch(base.agents, agents);
        }
        if let Some(ui) = patch.ui {
            base.ui = Self::apply_ui_patch(base.ui, ui);
        }
        if let Some(permissions) = patch.permissions {
            base.permissions = Self::apply_permission_patch(base.permissions, permissions);
        }
        if let Some(skills) = patch.skills {
            base.skills = Self::apply_skills_patch(base.skills, skills);
        }
        if let Some(storage) = patch.storage {
            base.storage = Self::apply_storage_patch(base.storage, storage);
        }
        if let Some(hooks) = patch.hooks {
            base.hooks = Self::merge_hooks(base.hooks, hooks);
        }
        if let Some(memory) = patch.memory {
            base.memory = Self::apply_memory_patch(base.memory, memory);
        }
        if let Some(logging) = patch.logging {
            base.logging = Self::apply_logging_patch(base.logging, logging);
        }
        if let Some(guidance) = patch.guidance {
            base.guidance = Self::apply_guidance_patch(base.guidance, guidance);
        }
        base
    }

    fn apply_api_patch(mut base: ApiConfig, patch: ApiConfigPatch) -> ApiConfig {
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

    fn apply_model_patch(mut base: ModelConfig, patch: ModelConfigPatch) -> ModelConfig {
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

    fn apply_models_patch(mut base: ModelsConfig, patch: ModelsConfigPatch) -> ModelsConfig {
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

    fn apply_tools_patch(mut base: ToolsConfig, patch: ToolsConfigPatch) -> ToolsConfig {
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
        base
    }

    fn apply_agents_patch(mut base: AgentsConfig, patch: AgentsConfigPatch) -> AgentsConfig {
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

    fn apply_ui_patch(mut base: UiConfig, patch: UiConfigPatch) -> UiConfig {
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
            base.task_list = Self::apply_task_list_patch(base.task_list, v);
        }
        if let Some(v) = patch.task_lifecycle {
            base.task_lifecycle = Self::apply_task_lifecycle_patch(base.task_lifecycle, v);
        }
        base
    }

    fn apply_task_list_patch(
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

    fn apply_task_lifecycle_patch(
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

    fn apply_permission_patch(
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

    fn apply_skills_patch(mut base: SkillsConfig, patch: SkillsConfigPatch) -> SkillsConfig {
        if let Some(v) = patch.dirs {
            base.dirs = v;
        }
        base
    }

    fn apply_storage_patch(mut base: StorageConfig, patch: StorageConfigPatch) -> StorageConfig {
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

    fn merge_hooks(base: HooksConfig, overlay: HooksConfig) -> HooksConfig {
        let mut events = base.events;
        for (k, v) in overlay.events {
            events.insert(k, v);
        }
        HooksConfig { events }
    }

    fn apply_memory_patch(mut base: MemoryConfig, patch: MemoryConfigPatch) -> MemoryConfig {
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
            base.reflection = Self::apply_reflection_patch(base.reflection, v);
        }
        base
    }

    fn apply_reflection_patch(
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

    fn apply_logging_patch(mut base: LoggingConfig, patch: LoggingConfigPatch) -> LoggingConfig {
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
            base.sub_agent_log = Self::apply_sub_agent_log_patch(base.sub_agent_log, v);
        }
        if let Some(v) = patch.logs_dir {
            base.logs_dir = Some(v);
        }
        if let Some(v) = patch.role_logs_enabled {
            base.role_logs_enabled = v;
        }
        base
    }

    fn apply_sub_agent_log_patch(
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

    fn apply_guidance_patch(
        mut base: GuidanceConfig,
        patch: GuidanceConfigPatch,
    ) -> GuidanceConfig {
        if let Some(v) = patch.reload_policy {
            base.reload_policy = match v.as_str() {
                "inject" => share::config::GuidanceReloadPolicy::Inject,
                "remind" => share::config::GuidanceReloadPolicy::Remind,
                "confirm" => share::config::GuidanceReloadPolicy::Confirm,
                _ => {
                    log::warn!(target: LOG_TARGET, "[config] unknown guidance.reload_policy '{}', keeping default", v);
                    base.reload_policy
                }
            };
        }
        base
    }
}

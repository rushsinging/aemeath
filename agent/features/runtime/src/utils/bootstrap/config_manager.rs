//! Runtime configuration manager — load, merge, and persist shared config DTOs.

use crate::utils::bootstrap::claude_settings_adapter::ClaudeSettingsAdapter;
use crate::utils::bootstrap::config_paths as paths;
use serde::Deserialize;
use serde_json::Value;
use share::config::{
    hooks::{self, HooksConfig},
    legacy::{ApiConfig, ModelConfig},
    logging::{LoggingConfig, SubAgentLogConfig},
    memory::{MemoryConfig, ReflectionConfig},
    models::{ModelsConfig, ProviderModelsConfig},
    paths as share_paths,
    permissions::{PermissionConfig, PermissionModeConfig},
    skills::SkillsConfig,
    storage::StorageConfig,
    tools::{AgentRoleConfig, AgentsConfig, ToolsConfig},
    ui::{TaskLifecycleConfig, TaskListConfig, UiConfig},
    Config, GuidanceConfig,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::sync::RwLock;

/// Configuration manager.
pub struct ConfigManager {
    /// Loaded configuration.
    config: RwLock<Config>,
    /// Global config file path.
    global_path: PathBuf,
    /// Project config file path.
    project_path: Option<PathBuf>,
    /// Claude Code project settings path.
    claude_project_settings_path: Option<PathBuf>,
}

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
    /// Create a new config manager.
    pub fn new(project_dir: Option<&Path>) -> Self {
        let global_path = paths::global_config_path();
        let project_path = project_dir.map(share_paths::project_config_path);
        let claude_project_settings_path =
            project_dir.map(share_paths::project_claude_settings_path);

        Self {
            config: RwLock::new(Config::default()),
            global_path,
            project_path,
            claude_project_settings_path,
        }
    }

    /// Load configuration from all sources.
    pub async fn load(&self) -> Result<Config, String> {
        let mut config = Config::default();

        // Load global config.
        if self.global_path.exists() {
            match tokio::fs::read_to_string(&self.global_path).await {
                Ok(content) => match serde_json::from_str::<ConfigPatch>(&content) {
                    Ok(global_patch) => config = Self::apply_patch(config, global_patch),
                    Err(err) => {
                        log::warn!(target: "runtime::config_manager", "解析全局配置失败 {}: {err}", self.global_path.display())
                    }
                },
                Err(err) => log::warn!(target: "runtime::config_manager", "读取全局配置失败 {}: {err}", self.global_path.display()),
            }
        }

        // Load Claude Code project settings as a lower-priority project fallback.
        if let Some(claude_path) = &self.claude_project_settings_path {
            if claude_path.exists() {
                match tokio::fs::read_to_string(claude_path).await {
                    Ok(content) => {
                        match serde_json::from_str::<hooks::ClaudeSettingsConfig>(&content) {
                            Ok(claude_config) => {
                                config = Self::apply_patch(
                                    config,
                                    ConfigPatch {
                                        hooks: Some(claude_config.into_config().hooks),
                                        ..Default::default()
                                    },
                                )
                            }
                            Err(err) => log::warn!(target: "runtime::config_manager",
                                "解析 Claude Code 项目设置失败 {}: {err}",
                                claude_path.display()
                            ),
                        }
                    }
                    Err(err) => log::warn!(target: "runtime::config_manager",
                        "读取 Claude Code 项目设置失败 {}: {err}",
                        claude_path.display()
                    ),
                }
            }
        }

        // Load project config.
        if let Some(project_path) = &self.project_path {
            if project_path.exists() {
                match tokio::fs::read_to_string(project_path).await {
                    Ok(content) => match serde_json::from_str::<ConfigPatch>(&content) {
                        Ok(project_patch) => config = Self::apply_patch(config, project_patch),
                        Err(err) => {
                            log::warn!(target: "runtime::config_manager", "解析项目配置失败 {}: {err}", project_path.display())
                        }
                    },
                    Err(err) => log::warn!(target: "runtime::config_manager", "读取项目配置失败 {}: {err}", project_path.display()),
                }
            }
        }

        config = Self::apply_env_vars(config);

        *self.config.write().await = config.clone();
        Ok(config)
    }

    /// Save configuration to global file.
    pub async fn save_global(&self) -> Result<(), String> {
        let config = self.config.read().await.clone();

        if let Some(parent) = self.global_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("创建配置目录失败: {e}"))?;
        }

        let content =
            serde_json::to_string_pretty(&config).map_err(|e| format!("序列化配置失败: {e}"))?;

        tokio::fs::write(&self.global_path, content)
            .await
            .map_err(|e| format!("写入配置失败: {e}"))?;

        Ok(())
    }

    /// Save configuration to project file.
    pub async fn save_project(&self) -> Result<(), String> {
        let project_path = self.project_path.as_ref().ok_or("未设置项目目录")?;
        let config = self.config.read().await.clone();

        if let Some(parent) = project_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("创建配置目录失败: {e}"))?;
        }

        let content =
            serde_json::to_string_pretty(&config).map_err(|e| format!("序列化配置失败: {e}"))?;

        tokio::fs::write(project_path, content)
            .await
            .map_err(|e| format!("写入配置失败: {e}"))?;

        Ok(())
    }

    /// Update configuration and persist it globally.
    pub async fn update<F>(&self, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut Config),
    {
        let mut config = self.config.write().await;
        f(&mut config);
        drop(config);
        self.save_global().await
    }

    /// Get current config.
    pub async fn get(&self) -> Config {
        self.config.read().await.clone()
    }

    /// Get global config path.
    pub fn global_path(&self) -> &Path {
        &self.global_path
    }

    /// Get project config path.
    pub fn project_path(&self) -> Option<&Path> {
        self.project_path.as_deref()
    }

    /// Get Claude Code project settings path.
    pub fn claude_project_settings_path(&self) -> Option<&Path> {
        self.claude_project_settings_path.as_deref()
    }

    /// Apply environment variable overrides.
    fn apply_env_vars(mut config: Config) -> Config {
        if let Ok(provider_str) = std::env::var("AEMEATH_PROVIDER") {
            config.api.provider = Some(provider_str);
        }

        if let Ok(key) = std::env::var("LLM_API_KEY") {
            config.api.key = Some(key);
        }

        if let Ok(url) = std::env::var("AEMEATH_BASE_URL") {
            config.api.base_url = Some(url);
        } else if let Ok(url) = std::env::var("LLM_BASE_URL") {
            config.api.base_url = Some(url);
        }

        if let Ok(model) = std::env::var("AEMEATH_MODEL") {
            config.model.name = model;
        }

        if let Ok(max_tokens) = std::env::var("AEMEATH_MAX_TOKENS") {
            if let Ok(val) = max_tokens.parse() {
                config.model.max_tokens = val;
            }
        }

        if let Ok(context_size) = std::env::var("AEMEATH_CONTEXT_SIZE") {
            if let Ok(val) = context_size.parse() {
                config.model.context_size = val;
            }
        }

        if let Ok(mode) = std::env::var("AEMEATH_PERMISSION_MODE") {
            match mode.to_lowercase().as_str() {
                "ask" => config.permissions.mode = PermissionModeConfig::Ask,
                "auto_read" | "autoread" => {
                    config.permissions.mode = PermissionModeConfig::AutoRead
                }
                "allow_all" | "auto_all" | "autoall" => {
                    config.permissions.mode = PermissionModeConfig::AllowAll
                }
                _ => {}
            }
        }

        if let Ok(val) = std::env::var("AEMEATH_MAX_TOOL_CONCURRENCY") {
            if let Ok(v) = val.parse::<usize>() {
                if v > 0 {
                    config.tools.max_concurrency = v;
                }
            }
        }

        if let Ok(val) = std::env::var("AEMEATH_MAX_AGENT_CONCURRENCY") {
            if let Ok(v) = val.parse::<usize>() {
                if v > 0 {
                    config.agents.max_concurrency = v;
                }
            }
        }

        if std::env::var("AEMEATH_VERBOSE").is_ok() {
            config.ui.verbose = true;
        }

        if std::env::var("NO_COLOR").is_ok() {
            config.ui.color = false;
        }

        config
    }

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
                    log::warn!(target: "runtime::config_manager", "[config] unknown guidance.reload_policy '{}', keeping default", v);
                    base.reload_policy
                }
            };
        }
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_patch_project_hooks_do_not_reset_global_logging_level() {
        let base = Config {
            logging: LoggingConfig {
                level: "debug".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let patch: ConfigPatch = serde_json::from_str(
            r#"{
              "hooks": {
                "Stop": [{ "command": "echo ok" }]
              }
            }"#,
        )
        .expect("project hooks patch should parse");

        let merged = ConfigManager::apply_patch(base, patch);

        assert_eq!(merged.logging.level, "debug");
        assert_eq!(merged.hooks.events.len(), 1);
    }

    #[test]
    fn test_apply_patch_project_can_explicitly_override_logging_level_to_warn() {
        let base = Config {
            logging: LoggingConfig {
                level: "debug".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };
        let patch: ConfigPatch = serde_json::from_str(
            r#"{
              "logging": { "level": "warn" }
            }"#,
        )
        .expect("logging patch should parse");

        let merged = ConfigManager::apply_patch(base, patch);

        assert_eq!(merged.logging.level, "warn");
    }

    #[test]
    fn test_apply_patch_missing_bool_fields_preserve_lower_priority_values() {
        let base = Config {
            ui: UiConfig {
                markdown: false,
                syntax_highlight: false,
                progress: false,
                color: false,
                verbose: true,
                tui: false,
                task_lifecycle: TaskLifecycleConfig {
                    auto_clear_completed_on_new_turn: false,
                    interrupt_prompt_enabled: false,
                    ..Default::default()
                },
                ..Default::default()
            },
            storage: StorageConfig {
                persist_sessions: false,
                history: false,
                ..Default::default()
            },
            memory: share::config::MemoryConfig {
                enabled: false,
                reflection: ReflectionConfig {
                    enabled: false,
                    auto_apply_suggestions: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            logging: LoggingConfig {
                sub_agent_log: SubAgentLogConfig {
                    enabled: false,
                    include_request_payload: false,
                    ..Default::default()
                },
                role_logs_enabled: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let patch: ConfigPatch = serde_json::from_str(r#"{}"#).expect("empty patch should parse");

        let merged = ConfigManager::apply_patch(base, patch);

        assert!(!merged.ui.markdown);
        assert!(!merged.ui.syntax_highlight);
        assert!(!merged.ui.progress);
        assert!(!merged.ui.color);
        assert!(merged.ui.verbose);
        assert!(!merged.ui.tui);
        assert!(!merged.ui.task_lifecycle.auto_clear_completed_on_new_turn);
        assert!(!merged.ui.task_lifecycle.interrupt_prompt_enabled);
        assert!(!merged.storage.persist_sessions);
        assert!(!merged.storage.history);
        assert!(!merged.memory.enabled);
        assert!(!merged.memory.reflection.enabled);
        assert!(merged.memory.reflection.auto_apply_suggestions);
        assert!(!merged.logging.sub_agent_log.enabled);
        assert!(!merged.logging.sub_agent_log.include_request_payload);
        assert!(!merged.logging.role_logs_enabled);
    }

    #[test]
    fn test_apply_patch_explicit_bool_fields_override_lower_priority_values() {
        let base = Config {
            ui: UiConfig {
                markdown: true,
                syntax_highlight: true,
                progress: true,
                color: true,
                verbose: true,
                tui: true,
                task_lifecycle: TaskLifecycleConfig {
                    auto_clear_completed_on_new_turn: true,
                    interrupt_prompt_enabled: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            storage: StorageConfig {
                persist_sessions: true,
                history: true,
                ..Default::default()
            },
            memory: share::config::MemoryConfig {
                enabled: true,
                reflection: ReflectionConfig {
                    enabled: true,
                    auto_apply_suggestions: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            logging: LoggingConfig {
                sub_agent_log: SubAgentLogConfig {
                    enabled: true,
                    include_request_payload: true,
                    ..Default::default()
                },
                role_logs_enabled: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let patch: ConfigPatch = serde_json::from_str(
            r#"{
              "ui": {
                "markdown": false,
                "syntax_highlight": false,
                "progress": false,
                "color": false,
                "verbose": false,
                "tui": false,
                "task_lifecycle": {
                  "auto_clear_completed_on_new_turn": false,
                  "interrupt_prompt_enabled": false
                }
              },
              "storage": {
                "persist_sessions": false,
                "history": false
              },
              "memory": {
                "enabled": false,
                "reflection": {
                  "enabled": false,
                  "auto_apply_suggestions": false
                }
              },
              "logging": {
                "sub_agent_log": {
                  "enabled": false,
                  "include_request_payload": false
                },
                "role_logs_enabled": false
              }
            }"#,
        )
        .expect("bool patch should parse");

        let merged = ConfigManager::apply_patch(base, patch);

        assert!(!merged.ui.markdown);
        assert!(!merged.ui.syntax_highlight);
        assert!(!merged.ui.progress);
        assert!(!merged.ui.color);
        assert!(!merged.ui.verbose);
        assert!(!merged.ui.tui);
        assert!(!merged.ui.task_lifecycle.auto_clear_completed_on_new_turn);
        assert!(!merged.ui.task_lifecycle.interrupt_prompt_enabled);
        assert!(!merged.storage.persist_sessions);
        assert!(!merged.storage.history);
        assert!(!merged.memory.enabled);
        assert!(!merged.memory.reflection.enabled);
        assert!(!merged.memory.reflection.auto_apply_suggestions);
        assert!(!merged.logging.sub_agent_log.enabled);
        assert!(!merged.logging.sub_agent_log.include_request_payload);
        assert!(!merged.logging.role_logs_enabled);
    }

    use crate::utils::bootstrap::config_paths::TestEnvGuard;

    #[tokio::test]
    async fn test_load_project_hooks_do_not_reset_global_logging_level() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("home_agents");
        let project = root.path().join("project");
        let project_agents = project.join(".agents");
        tokio::fs::create_dir_all(&home).await.unwrap();
        tokio::fs::create_dir_all(&project_agents).await.unwrap();
        tokio::fs::write(
            home.join("aemeath.json"),
            r#"{ "logging": { "level": "debug" } }"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            project_agents.join("aemeath.json"),
            r#"{ "hooks": { "Stop": [{ "command": "echo ok" }] } }"#,
        )
        .await
        .unwrap();

        let _guard = TestEnvGuard::set("AEMEATH_AGENTS_DIR", home.to_string_lossy().as_ref());
        let manager = ConfigManager::new(Some(&project));

        let loaded = manager.load().await.expect("config should load");

        assert_eq!(loaded.logging.level, "debug");
        assert_eq!(loaded.hooks.events.len(), 1);
    }

    #[tokio::test]
    async fn test_load_does_not_inject_builtin_model_providers() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("home_agents");
        let project = root.path().join("project");
        tokio::fs::create_dir_all(&home).await.unwrap();
        tokio::fs::create_dir_all(&project).await.unwrap();

        let _guard = TestEnvGuard::set("AEMEATH_AGENTS_DIR", home.to_string_lossy().as_ref());
        let manager = ConfigManager::new(Some(&project));

        let loaded = manager.load().await.expect("config should load");

        assert!(loaded.models.providers.is_empty());
        assert!(loaded.models.default.is_empty());
    }

    #[tokio::test]
    async fn test_load_project_hooks_do_not_reset_global_models() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("home_agents");
        let project = root.path().join("project");
        let project_agents = project.join(".agents");
        tokio::fs::create_dir_all(&home).await.unwrap();
        tokio::fs::create_dir_all(&project_agents).await.unwrap();
        tokio::fs::write(
            home.join("aemeath.json"),
            r#"{
              "models": {
                "default": "MiniMax/MiniMax-M3",
                "providers": {
                  "MiniMax": {
                    "baseUrl": "https://api.minimaxi.com/v1",
                    "apiKey": "minimax-key",
                    "driver": "minimax",
                    "models": [{ "id": "MiniMax-M3", "name": "MiniMax-M3" }]
                  }
                }
              }
            }"#,
        )
        .await
        .unwrap();
        tokio::fs::write(
            project_agents.join("aemeath.json"),
            r#"{ "hooks": { "Stop": [{ "command": "echo ok" }] } }"#,
        )
        .await
        .unwrap();

        let _guard = TestEnvGuard::set("AEMEATH_AGENTS_DIR", home.to_string_lossy().as_ref());
        let manager = ConfigManager::new(Some(&project));

        let loaded = manager.load().await.expect("config should load");
        let provider = loaded
            .models
            .providers
            .get("MiniMax")
            .expect("global provider should remain configured");

        assert_eq!(loaded.models.providers.len(), 1);
        assert_eq!(loaded.models.default, "MiniMax/MiniMax-M3");
        assert_eq!(provider.api_key, "minimax-key");
        assert!(!loaded.models.providers.contains_key("Minimax"));
    }
}

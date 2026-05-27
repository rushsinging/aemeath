//! Runtime configuration manager — load, merge, and persist shared config DTOs.

use crate::api::core::config::{
    hooks::{self, HooksConfig},
    legacy::{ApiConfig, ModelConfig},
    logging::{LoggingConfig, SubAgentLogConfig},
    models::{volcengine_coding_plan_config, ModelsConfig},
    paths as share_paths,
    permissions::{PermissionConfig, PermissionModeConfig},
    skills::SkillsConfig,
    storage::StorageConfig,
    tools::{AgentsConfig, ToolsConfig},
    ui::{TaskLifecycleConfig, TaskListConfig, UiConfig},
    Config,
};
use crate::utils::bootstrap::claude_settings_adapter::ClaudeSettingsAdapter;
use crate::utils::bootstrap::config_paths as paths;
use std::path::{Path, PathBuf};
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

fn default_api_config() -> ApiConfig {
    ApiConfig::default()
}

fn default_model_config() -> ModelConfig {
    ModelConfig::default()
}

fn default_tools_config() -> ToolsConfig {
    ToolsConfig::default()
}

fn default_agents_config() -> AgentsConfig {
    AgentsConfig::default()
}

fn default_logging_config() -> LoggingConfig {
    LoggingConfig::default()
}

fn default_sub_agent_log_config() -> SubAgentLogConfig {
    SubAgentLogConfig::default()
}

impl ConfigManager {
    /// Create a new config manager.
    pub fn new(project_dir: Option<&Path>) -> Self {
        let global_path = paths::global_config_path();
        let project_path = project_dir.map(share_paths::project_config_path);
        let claude_project_settings_path = project_dir.map(share_paths::project_claude_settings_path);

        Self {
            config: RwLock::new(Config::default()),
            global_path,
            project_path,
            claude_project_settings_path,
        }
    }

    /// Load configuration from all sources.
    pub async fn load(&self) -> Result<Config, String> {
        let mut config = Config {
            models: volcengine_coding_plan_config(),
            ..Default::default()
        };

        // Load global config.
        if self.global_path.exists() {
            match tokio::fs::read_to_string(&self.global_path).await {
                Ok(content) => match serde_json::from_str::<Config>(&content) {
                    Ok(global_config) => config = Self::merge_config(config, global_config),
                    Err(err) => log::warn!("解析全局配置失败 {}: {err}", self.global_path.display()),
                },
                Err(err) => log::warn!("读取全局配置失败 {}: {err}", self.global_path.display()),
            }
        }

        // Load Claude Code project settings as a lower-priority project fallback.
        if let Some(claude_path) = &self.claude_project_settings_path {
            if claude_path.exists() {
                match tokio::fs::read_to_string(claude_path).await {
                    Ok(content) => match serde_json::from_str::<hooks::ClaudeSettingsConfig>(&content)
                    {
                        Ok(claude_config) => {
                            config = Self::merge_config(config, claude_config.into_config())
                        }
                        Err(err) => log::warn!(
                            "解析 Claude Code 项目设置失败 {}: {err}",
                            claude_path.display()
                        ),
                    },
                    Err(err) => log::warn!(
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
                    Ok(content) => match serde_json::from_str::<Config>(&content) {
                        Ok(project_config) => config = Self::merge_config(config, project_config),
                        Err(err) => log::warn!("解析项目配置失败 {}: {err}", project_path.display()),
                    },
                    Err(err) => log::warn!("读取项目配置失败 {}: {err}", project_path.display()),
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
                "auto_read" | "autoread" => config.permissions.mode = PermissionModeConfig::AutoRead,
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

    /// Merge two configs (overlay takes precedence).
    pub(crate) fn merge_config(base: Config, overlay: Config) -> Config {
        Config {
            api: ApiConfig {
                provider: overlay.api.provider.or(base.api.provider),
                key: overlay.api.key.or(base.api.key),
                base_url: overlay.api.base_url.or(base.api.base_url),
                user_agent: if overlay.api.user_agent != default_api_config().user_agent {
                    overlay.api.user_agent
                } else {
                    base.api.user_agent
                },
                timeout: if overlay.api.timeout != default_api_config().timeout {
                    overlay.api.timeout
                } else {
                    base.api.timeout
                },
                retries: if overlay.api.retries != default_api_config().retries {
                    overlay.api.retries
                } else {
                    base.api.retries
                },
            },
            model: ModelConfig {
                name: if overlay.model.name != default_model_config().name {
                    overlay.model.name
                } else {
                    base.model.name
                },
                max_tokens: if overlay.model.max_tokens != default_model_config().max_tokens {
                    overlay.model.max_tokens
                } else {
                    base.model.max_tokens
                },
                context_size: if overlay.model.context_size != default_model_config().context_size {
                    overlay.model.context_size
                } else {
                    base.model.context_size
                },
                temperature: overlay.model.temperature.or(base.model.temperature),
                top_k: overlay.model.top_k.or(base.model.top_k),
                top_p: overlay.model.top_p.or(base.model.top_p),
                stop_sequences: if !overlay.model.stop_sequences.is_empty() {
                    overlay.model.stop_sequences
                } else {
                    base.model.stop_sequences
                },
            },
            models: {
                let mut providers = base.models.providers;
                for (k, v) in overlay.models.providers {
                    providers.insert(k, v);
                }
                let mut guidance = base.models.guidance;
                for (k, v) in overlay.models.guidance {
                    guidance.insert(k, v);
                }
                ModelsConfig {
                    mode: if overlay.models.mode.is_empty() {
                        base.models.mode
                    } else {
                        overlay.models.mode
                    },
                    default: if overlay.models.default.is_empty() {
                        base.models.default
                    } else {
                        overlay.models.default
                    },
                    providers,
                    guidance,
                }
            },
            tools: ToolsConfig {
                enabled: if !overlay.tools.enabled.is_empty() {
                    overlay.tools.enabled
                } else {
                    base.tools.enabled
                },
                disabled: if !overlay.tools.disabled.is_empty() {
                    overlay.tools.disabled
                } else {
                    base.tools.disabled
                },
                settings: Self::merge_maps(base.tools.settings, overlay.tools.settings),
                max_concurrency: if overlay.tools.max_concurrency != default_tools_config().max_concurrency
                {
                    overlay.tools.max_concurrency
                } else {
                    base.tools.max_concurrency
                },
            },
            agents: AgentsConfig {
                max_concurrency: if overlay.agents.max_concurrency
                    != default_agents_config().max_concurrency
                {
                    overlay.agents.max_concurrency
                } else {
                    base.agents.max_concurrency
                },
                roles: {
                    let mut roles = base.agents.roles;
                    for (k, v) in overlay.agents.roles {
                        roles.insert(k, v);
                    }
                    roles
                },
                default_model: if !overlay.agents.default_model.is_empty() {
                    overlay.agents.default_model
                } else {
                    base.agents.default_model
                },
            },
            ui: UiConfig {
                markdown: overlay.ui.markdown,
                syntax_highlight: overlay.ui.syntax_highlight,
                progress: overlay.ui.progress,
                color: overlay.ui.color,
                verbose: overlay.ui.verbose || base.ui.verbose,
                tui: overlay.ui.tui,
                task_list: TaskListConfig {
                    max_lines: if overlay.ui.task_list.max_lines != 0 {
                        overlay.ui.task_list.max_lines
                    } else {
                        base.ui.task_list.max_lines
                    },
                    fold_hint_format: if !overlay.ui.task_list.fold_hint_format.is_empty() {
                        overlay.ui.task_list.fold_hint_format
                    } else {
                        base.ui.task_list.fold_hint_format
                    },
                },
                task_lifecycle: TaskLifecycleConfig {
                    auto_clear_completed_on_new_turn: overlay
                        .ui
                        .task_lifecycle
                        .auto_clear_completed_on_new_turn,
                    interrupt_prompt_enabled: overlay.ui.task_lifecycle.interrupt_prompt_enabled,
                    interrupt_default_action: if !overlay
                        .ui
                        .task_lifecycle
                        .interrupt_default_action
                        .is_empty()
                    {
                        overlay.ui.task_lifecycle.interrupt_default_action
                    } else {
                        base.ui.task_lifecycle.interrupt_default_action
                    },
                    stale_remind_after_turns: if overlay.ui.task_lifecycle.stale_remind_after_turns != 0
                    {
                        overlay.ui.task_lifecycle.stale_remind_after_turns
                    } else {
                        base.ui.task_lifecycle.stale_remind_after_turns
                    },
                    stale_remind_repeat_interval: if overlay
                        .ui
                        .task_lifecycle
                        .stale_remind_repeat_interval
                        != 0
                    {
                        overlay.ui.task_lifecycle.stale_remind_repeat_interval
                    } else {
                        base.ui.task_lifecycle.stale_remind_repeat_interval
                    },
                },
            },
            permissions: PermissionConfig {
                mode: if overlay.permissions.mode != PermissionModeConfig::default() {
                    overlay.permissions.mode
                } else {
                    base.permissions.mode
                },
                auto_approve: if !overlay.permissions.auto_approve.is_empty() {
                    overlay.permissions.auto_approve
                } else {
                    base.permissions.auto_approve
                },
                deny: if !overlay.permissions.deny.is_empty() {
                    overlay.permissions.deny
                } else {
                    base.permissions.deny
                },
            },
            storage: StorageConfig {
                sessions_dir: overlay.storage.sessions_dir.or(base.storage.sessions_dir),
                persist_sessions: overlay.storage.persist_sessions,
                max_sessions: overlay.storage.max_sessions,
                history: overlay.storage.history,
                history_file: overlay.storage.history_file.or(base.storage.history_file),
            },
            skills: SkillsConfig {
                dirs: if !overlay.skills.dirs.is_empty() {
                    overlay.skills.dirs
                } else {
                    base.skills.dirs
                },
            },
            hooks: {
                let mut events = base.hooks.events;
                for (k, v) in overlay.hooks.events {
                    events.insert(k, v);
                }
                HooksConfig { events }
            },
            memory: overlay.memory,
            logging: LoggingConfig {
                level: if !overlay.logging.level.is_empty() {
                    overlay.logging.level
                } else {
                    base.logging.level
                },
                module_levels: serde_json::Value::Null,
                max_bytes: if overlay.logging.max_bytes != default_logging_config().max_bytes {
                    overlay.logging.max_bytes
                } else {
                    base.logging.max_bytes
                },
                max_backups: if overlay.logging.max_backups != default_logging_config().max_backups {
                    overlay.logging.max_backups
                } else {
                    base.logging.max_backups
                },
                retention_days: if overlay.logging.retention_days
                    != default_logging_config().retention_days
                {
                    overlay.logging.retention_days
                } else {
                    base.logging.retention_days
                },
                sub_agent_log: SubAgentLogConfig {
                    enabled: overlay.logging.sub_agent_log.enabled,
                    include_request_payload: overlay.logging.sub_agent_log.include_request_payload,
                    max_payload_bytes: if overlay.logging.sub_agent_log.max_payload_bytes
                        != default_sub_agent_log_config().max_payload_bytes
                    {
                        overlay.logging.sub_agent_log.max_payload_bytes
                    } else {
                        base.logging.sub_agent_log.max_payload_bytes
                    },
                },
                logs_dir: overlay.logging.logs_dir.or(base.logging.logs_dir.clone()),
                role_logs_enabled: overlay.logging.role_logs_enabled,
            },
        }
    }

    /// Merge two hashmaps.
    pub(crate) fn merge_maps(
        base: std::collections::HashMap<String, serde_json::Value>,
        overlay: std::collections::HashMap<String, serde_json::Value>,
    ) -> std::collections::HashMap<String, serde_json::Value> {
        let mut result = base;
        result.extend(overlay);
        result
    }
}

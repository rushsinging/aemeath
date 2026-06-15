//! Runtime configuration manager — load, merge, and persist shared config DTOs.

use crate::utils::bootstrap::claude_settings_adapter::ClaudeSettingsAdapter;
use crate::utils::bootstrap::config_patch::ConfigPatch;
use crate::utils::bootstrap::config_paths as paths;
use share::config::{hooks, paths as share_paths, permissions::PermissionModeConfig, Config};
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

#[cfg(test)]
use share::config::{
    logging::{LoggingConfig, SubAgentLogConfig},
    memory::ReflectionConfig,
    storage::StorageConfig,
    ui::{TaskLifecycleConfig, UiConfig},
};

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
                Err(err) => {
                    log::warn!(target: "runtime::config_manager", "读取全局配置失败 {}: {err}", self.global_path.display())
                }
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
                                    ConfigPatch::with_hooks(claude_config.into_config().hooks),
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
                    Err(err) => {
                        log::warn!(target: "runtime::config_manager", "读取项目配置失败 {}: {err}", project_path.display())
                    }
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

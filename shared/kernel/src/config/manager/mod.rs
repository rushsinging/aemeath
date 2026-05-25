//! 配置管理器 — 加载、合并、保存配置

mod merge;
mod persistence;

use super::*;
use crate::config::paths;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

/// Configuration manager
pub struct ConfigManager {
    /// Loaded configuration
    config: RwLock<Config>,
    /// Global config file path
    global_path: PathBuf,
    /// Project config file path
    project_path: Option<PathBuf>,
    /// Claude Code project settings path
    claude_project_settings_path: Option<PathBuf>,
}

impl ConfigManager {
    /// Create a new config manager
    pub fn new(project_dir: Option<&Path>) -> Self {
        let global_path = paths::global_config_path();
        let project_path = project_dir.map(paths::project_config_path);
        let claude_project_settings_path = project_dir.map(paths::project_claude_settings_path);

        Self {
            config: RwLock::new(Config::default()),
            global_path,
            project_path,
            claude_project_settings_path,
        }
    }

    /// Load configuration from all sources
    pub async fn load(&self) -> Result<Config, String> {
        let mut config = Config {
            models: volcengine_coding_plan_config(),
            ..Default::default()
        };

        // Load global config
        if self.global_path.exists() {
            match tokio::fs::read_to_string(&self.global_path).await {
                Ok(content) => match serde_json::from_str::<Config>(&content) {
                    Ok(global_config) => config = Self::merge_config(config, global_config),
                    Err(err) => {
                        log::warn!("解析全局配置失败 {}: {err}", self.global_path.display())
                    }
                },
                Err(err) => log::warn!("读取全局配置失败 {}: {err}", self.global_path.display()),
            }
        }

        // Load Claude Code project settings as a lower-priority project fallback
        if let Some(claude_path) = &self.claude_project_settings_path {
            if claude_path.exists() {
                match tokio::fs::read_to_string(claude_path).await {
                    Ok(content) => {
                        match serde_json::from_str::<hooks::ClaudeSettingsConfig>(&content) {
                            Ok(claude_config) => {
                                config = Self::merge_config(config, claude_config.into_config())
                            }
                            Err(err) => log::warn!(
                                "解析 Claude Code 项目设置失败 {}: {err}",
                                claude_path.display()
                            ),
                        }
                    }
                    Err(err) => log::warn!(
                        "读取 Claude Code 项目设置失败 {}: {err}",
                        claude_path.display()
                    ),
                }
            }
        }

        // Load project config
        if let Some(project_path) = &self.project_path {
            if project_path.exists() {
                match tokio::fs::read_to_string(project_path).await {
                    Ok(content) => match serde_json::from_str::<Config>(&content) {
                        Ok(project_config) => config = Self::merge_config(config, project_config),
                        Err(err) => {
                            log::warn!("解析项目配置失败 {}: {err}", project_path.display())
                        }
                    },
                    Err(err) => log::warn!("读取项目配置失败 {}: {err}", project_path.display()),
                }
            }
        }

        // Override with environment variables
        config = Self::apply_env_vars(config);

        *self.config.write().await = config.clone();
        Ok(config)
    }

    /// Apply environment variable overrides
    fn apply_env_vars(mut config: Config) -> Config {
        // Legacy provider env var
        if let Ok(provider_str) = std::env::var("AEMEATH_PROVIDER") {
            config.api.provider = Some(provider_str);
        }

        // API key - check driver-specific env var first, then generic
        if let Ok(key) = std::env::var("LLM_API_KEY") {
            config.api.key = Some(key);
        }

        // Base URL
        if let Ok(url) = std::env::var("AEMEATH_BASE_URL") {
            config.api.base_url = Some(url);
        } else if let Ok(url) = std::env::var("LLM_BASE_URL") {
            config.api.base_url = Some(url);
        }

        // Model
        if let Ok(model) = std::env::var("AEMEATH_MODEL") {
            config.model.name = model;
        }

        // Max tokens
        if let Ok(max_tokens) = std::env::var("AEMEATH_MAX_TOKENS") {
            if let Ok(val) = max_tokens.parse() {
                config.model.max_tokens = val;
            }
        }

        // Context size
        if let Ok(context_size) = std::env::var("AEMEATH_CONTEXT_SIZE") {
            if let Ok(val) = context_size.parse() {
                config.model.context_size = val;
            }
        }

        // Permission mode
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

        // Max tool concurrency
        if let Ok(val) = std::env::var("AEMEATH_MAX_TOOL_CONCURRENCY") {
            if let Ok(v) = val.parse::<usize>() {
                if v > 0 {
                    config.tools.max_concurrency = v;
                }
            }
        }

        // Max agent concurrency
        if let Ok(val) = std::env::var("AEMEATH_MAX_AGENT_CONCURRENCY") {
            if let Ok(v) = val.parse::<usize>() {
                if v > 0 {
                    config.agents.max_concurrency = v;
                }
            }
        }

        // Verbose
        if std::env::var("AEMEATH_VERBOSE").is_ok() {
            config.ui.verbose = true;
        }

        // No color
        if std::env::var("NO_COLOR").is_ok() {
            config.ui.color = false;
        }

        config
    }

    /// Get current config
    pub async fn get(&self) -> Config {
        self.config.read().await.clone()
    }

    /// Get global config path
    pub fn global_path(&self) -> &Path {
        &self.global_path
    }

    /// Get project config path
    pub fn project_path(&self) -> Option<&Path> {
        self.project_path.as_deref()
    }

    /// Get Claude Code project settings path
    pub fn claude_project_settings_path(&self) -> Option<&Path> {
        self.claude_project_settings_path.as_deref()
    }
}

#[cfg(test)]
mod tests;

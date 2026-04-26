//! 配置管理器 — 加载、合并、保存配置

use super::*;
use crate::provider::Provider;
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
}

impl ConfigManager {
    /// Create a new config manager
    pub fn new(project_dir: Option<&Path>) -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let global_path = home.join(".aemeath").join("config.json");
        let project_path = project_dir.map(|p| p.join(".aemeath").join("config.json"));

        Self {
            config: RwLock::new(Config::default()),
            global_path,
            project_path,
        }
    }

    /// Load configuration from all sources
    pub async fn load(&self) -> Result<Config, String> {
        let mut config = Config::default();

        // Load global config
        if self.global_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&self.global_path).await {
                if let Ok(global_config) = serde_json::from_str::<Config>(&content) {
                    config = Self::merge_config(config, global_config);
                }
            }
        }

        // Load project config
        if let Some(project_path) = &self.project_path {
            if project_path.exists() {
                if let Ok(content) = tokio::fs::read_to_string(project_path).await {
                    if let Ok(project_config) = serde_json::from_str::<Config>(&content) {
                        config = Self::merge_config(config, project_config);
                    }
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
        // Provider
        if let Ok(provider_str) = std::env::var("AEMEATH_PROVIDER") {
            if let Some(provider) = Provider::from_str(&provider_str) {
                config.api.provider = Some(provider);
            }
        }

        // API key - check provider-specific env var first
        let effective_provider = config.api.provider.unwrap_or_default();
        let provider_key_env = effective_provider.api_key_env();
        if let Ok(key) = std::env::var(provider_key_env) {
            config.api.key = Some(key);
        } else if let Ok(key) = std::env::var("LLM_API_KEY") {
            // Generic fallback (provider-agnostic)
            config.api.key = Some(key);
        }

        // Base URL - check provider-specific env var first
        let provider_base_env = effective_provider.base_url_env();
        if let Ok(url) = std::env::var("AEMEATH_BASE_URL") {
            config.api.base_url = Some(url);
        } else if let Ok(url) = std::env::var(provider_base_env) {
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
                "auto_read" | "autoread" => config.permissions.mode = PermissionModeConfig::AutoRead,
                "allow_all" | "auto_all" | "autoall" => config.permissions.mode = PermissionModeConfig::AllowAll,
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

    /// Merge two configs (overlay takes precedence)
    fn merge_config(base: Config, overlay: Config) -> Config {
        Config {
            api: ApiConfig {
                // None = unset, use base value; Some = explicitly set
                provider: overlay.api.provider.or(base.api.provider),
                key: overlay.api.key.or(base.api.key),
                base_url: overlay.api.base_url.or(base.api.base_url),
                user_agent: if overlay.api.user_agent != legacy::default_user_agent() {
                    overlay.api.user_agent
                } else {
                    base.api.user_agent
                },
                timeout: if overlay.api.timeout != legacy::default_timeout() {
                    overlay.api.timeout
                } else {
                    base.api.timeout
                },
                retries: if overlay.api.retries != legacy::default_retries() {
                    overlay.api.retries
                } else {
                    base.api.retries
                },
            },
            model: ModelConfig {
                name: if overlay.model.name != legacy::default_model() {
                    overlay.model.name
                } else {
                    base.model.name
                },
                max_tokens: if overlay.model.max_tokens != legacy::default_max_tokens() {
                    overlay.model.max_tokens
                } else {
                    base.model.max_tokens
                },
                context_size: if overlay.model.context_size != legacy::default_context_size() {
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
                // Merge providers from both configs
                let mut providers = base.models.providers;
                for (k, v) in overlay.models.providers {
                    providers.insert(k, v);
                }
                // Merge guidance from both configs
                let mut guidance = base.models.guidance;
                for (k, v) in overlay.models.guidance {
                    guidance.insert(k, v);
                }
                ModelsConfig {
                    mode: if overlay.models.mode.is_empty() { base.models.mode } else { overlay.models.mode },
                    default: if overlay.models.default.is_empty() { base.models.default } else { overlay.models.default },
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
                max_concurrency: if overlay.tools.max_concurrency != tools::default_max_tool_concurrency() {
                    overlay.tools.max_concurrency
                } else {
                    base.tools.max_concurrency
                },
            },
            agents: AgentsConfig {
                max_concurrency: if overlay.agents.max_concurrency != tools::default_max_agent_concurrency() {
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
                // For boolean/numeric fields we cannot distinguish "unset" from "set to default"
                // using serde defaults. Use overlay directly — the user chose these values.
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
                // Merge hooks: overlay takes precedence for each event type
                let mut events = base.hooks.events;
                for (k, v) in overlay.hooks.events {
                    events.insert(k, v);
                }
                HooksConfig { events }
            },
        }
    }

    /// Merge two hashmaps
    fn merge_maps(
        base: std::collections::HashMap<String, serde_json::Value>,
        overlay: std::collections::HashMap<String, serde_json::Value>,
    ) -> std::collections::HashMap<String, serde_json::Value> {
        let mut result = base;
        result.extend(overlay);
        result
    }

    /// Get current config
    pub async fn get(&self) -> Config {
        self.config.read().await.clone()
    }

    /// Save configuration to global file
    pub async fn save_global(&self) -> Result<(), String> {
        let config = self.config.read().await.clone();

        // Ensure parent directory exists
        if let Some(parent) = self.global_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create config directory: {e}"))?;
        }

        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;

        tokio::fs::write(&self.global_path, content)
            .await
            .map_err(|e| format!("Failed to write config: {e}"))?;

        Ok(())
    }

    /// Save configuration to project file
    pub async fn save_project(&self) -> Result<(), String> {
        let project_path = self
            .project_path
            .as_ref()
            .ok_or("No project directory set")?;

        let config = self.config.read().await.clone();

        // Ensure parent directory exists
        if let Some(parent) = project_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create config directory: {e}"))?;
        }

        let content = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;

        tokio::fs::write(project_path, content)
            .await
            .map_err(|e| format!("Failed to write config: {e}"))?;

        Ok(())
    }

    /// Update configuration
    pub async fn update<F>(&self, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut Config),
    {
        let mut config = self.config.write().await;
        f(&mut config);
        drop(config);
        self.save_global().await
    }

    /// Get global config path
    pub fn global_path(&self) -> &Path {
        &self.global_path
    }

    /// Get project config path
    pub fn project_path(&self) -> Option<&Path> {
        self.project_path.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::hooks::{HookEntry, HookEvent, HooksConfig};
    use std::collections::HashMap;

    /// Helper: build a `Config` with the given hooks (everything else default).
    fn config_with_hooks(events: HashMap<HookEvent, Vec<HookEntry>>) -> Config {
        let mut cfg = Config::default();
        cfg.hooks = HooksConfig { events };
        cfg
    }

    /// Helper: build a single `HookEntry`.
    fn hook_entry(matcher: &str, command: &str) -> HookEntry {
        HookEntry {
            matcher: matcher.to_string(),
            command: command.to_string(),
            timeout: 30,
        }
    }

    // ---- Test 1: hooks 空合并（base 和 overlay 都没有 hooks） ----

    #[test]
    fn test_merge_hooks_both_empty() {
        let base = Config::default();
        let overlay = Config::default();

        let merged = ConfigManager::merge_config(base, overlay);

        assert!(merged.hooks.events.is_empty());
    }

    // ---- Test 2: overlay 覆盖 base hooks（同事件类型） ----

    #[test]
    fn test_merge_hooks_overlay_overrides_same_event() {
        let base = config_with_hooks(HashMap::from([(
            HookEvent::PreToolUse,
            vec![hook_entry("Bash", "base-hook")],
        )]));

        let overlay = config_with_hooks(HashMap::from([(
            HookEvent::PreToolUse,
            vec![hook_entry("Bash", "overlay-hook"), hook_entry("Read", "overlay-read")],
        )]));

        let merged = ConfigManager::merge_config(base, overlay);

        let pre = merged.hooks.events.get(&HookEvent::PreToolUse).unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0].command, "overlay-hook");
        assert_eq!(pre[1].command, "overlay-read");
        // base hook should NOT appear
        assert!(!pre.iter().any(|h| h.command == "base-hook"));
    }

    // ---- Test 3: base 有 hooks, overlay 没有 → 保留 base ----

    #[test]
    fn test_merge_hooks_base_only_preserved() {
        let base = config_with_hooks(HashMap::from([(
            HookEvent::PostToolUse,
            vec![hook_entry("", "post-hook")],
        )]));
        let overlay = Config::default();

        let merged = ConfigManager::merge_config(base, overlay);

        let post = merged.hooks.events.get(&HookEvent::PostToolUse).unwrap();
        assert_eq!(post.len(), 1);
        assert_eq!(post[0].command, "post-hook");
    }

    // ---- Test 4: overlay 新增不同事件类型 → 两者都保留 ----

    #[test]
    fn test_merge_hooks_overlay_adds_new_event() {
        let base = config_with_hooks(HashMap::from([(
            HookEvent::PreToolUse,
            vec![hook_entry("Bash", "pre-hook")],
        )]));

        let overlay = config_with_hooks(HashMap::from([(
            HookEvent::Stop,
            vec![hook_entry("", "stop-hook")],
        )]));

        let merged = ConfigManager::merge_config(base, overlay);

        assert_eq!(merged.hooks.events.len(), 2);

        let pre = merged.hooks.events.get(&HookEvent::PreToolUse).unwrap();
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0].command, "pre-hook");

        let stop = merged.hooks.events.get(&HookEvent::Stop).unwrap();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0].command, "stop-hook");
    }

    // ---- Test 5: Config 整体 hooks 字段 JSON 反序列化 ----

    #[test]
    fn test_config_hooks_deserialize() {
        let json = r#"{
            "hooks": {
                "PreToolUse": [
                    { "matcher": "Bash", "command": "echo before-bash" }
                ],
                "Stop": [
                    { "matcher": "", "command": "echo stopped", "timeout": 60 }
                ]
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();

        assert_eq!(config.hooks.events.len(), 2);

        let pre = config.hooks.events.get(&HookEvent::PreToolUse).unwrap();
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0].matcher, "Bash");
        assert_eq!(pre[0].command, "echo before-bash");
        assert_eq!(pre[0].timeout, 30); // default

        let stop = config.hooks.events.get(&HookEvent::Stop).unwrap();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0].command, "echo stopped");
        assert_eq!(stop[0].timeout, 60); // explicit
    }
}

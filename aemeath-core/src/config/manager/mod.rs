//! 配置管理器 — 加载、合并、保存配置

mod merge;
mod persistence;

use super::*;
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
            vec![
                hook_entry("Bash", "overlay-hook"),
                hook_entry("Read", "overlay-read"),
            ],
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

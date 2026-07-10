//! ConfigAppService — orchestrates adapter chain, holds snapshot, implements ConfigReader.
//!
//! Lives in runtime feature (not share kernel) because it performs
//! fs IO and holds stateful RwLock + watch::Sender.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use share::config::adapter::env as env_adapter;
use share::config::domain::merge::{ConfigPatch, PriorityChain};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use tokio::sync::{watch, RwLock};

use super::config_port::ConfigReader;

#[allow(dead_code)] // used in S2 when logging is added
const LOG_TARGET: &str = "aemeath:agent:runtime";

/// Application service for configuration management.
///
/// Adapters produce `ConfigPatch` from each source; `PriorityChain`
/// merges them in priority order; `watch::Sender` pushes new snapshots.
pub struct ConfigAppService {
    tx: watch::Sender<ConfigSnapshot>,
    inner: RwLock<Inner>,
}

struct Inner {
    config: Config,
    global_path: PathBuf,
    project_path: Option<PathBuf>,
    claude_project_settings_path: Option<PathBuf>,
    cli_patch: ConfigPatch,
}

impl ConfigAppService {
    pub fn new(project_dir: Option<&Path>) -> Self {
        Self::with_global_path(project_dir, share::config::paths::global_config_path())
    }

    /// Create with a custom global config path (for testing).
    pub(crate) fn with_global_path(project_dir: Option<&Path>, global_path: PathBuf) -> Self {
        let project_path = project_dir.map(share::config::paths::project_config_path);
        let claude_project_settings_path =
            project_dir.map(share::config::paths::project_claude_settings_path);

        let initial = Config::default();
        let snapshot = ConfigSnapshot::new(initial.clone());
        let (tx, _) = watch::channel(snapshot);

        Self {
            tx,
            inner: RwLock::new(Inner {
                config: initial,
                global_path,
                project_path,
                claude_project_settings_path,
                cli_patch: ConfigPatch::default(),
            }),
        }
    }

    /// Set the CLI args patch (highest priority source).
    pub async fn set_cli_patch(&self, patch: ConfigPatch) {
        self.inner.write().await.cli_patch = patch;
    }

    /// Load from all sources, building the priority chain.
    pub async fn load(&self) -> Result<Config, String> {
        let inner = self.inner.read().await;

        let mut chain = PriorityChain::new();

        // Claude Settings (ACL)
        if let Some(path) = &inner.claude_project_settings_path {
            if path.exists() {
                if let Ok(content) = tokio::fs::read_to_string(path).await {
                    if let Ok(patch) = serde_json::from_str::<ConfigPatch>(&content) {
                        chain.push(patch);
                    }
                }
            }
        }

        // Global file
        if inner.global_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&inner.global_path).await {
                if let Ok(patch) = serde_json::from_str::<ConfigPatch>(&content) {
                    chain.push(patch);
                }
            }
        }

        // Project file
        if let Some(path) = &inner.project_path {
            if path.exists() {
                if let Ok(content) = tokio::fs::read_to_string(path).await {
                    if let Ok(patch) = serde_json::from_str::<ConfigPatch>(&content) {
                        chain.push(patch);
                    }
                }
            }
        }

        // Env
        let env_patch = env_adapter::read();
        if !env_patch.is_empty() {
            chain.push(env_patch);
        }

        // CLI args (highest priority)
        if !inner.cli_patch.is_empty() {
            chain.push(inner.cli_patch.clone());
        }

        drop(inner);

        let mut config = chain.merge(Config::default());

        // Per-provider API key 后处理：对 api_key 为空的 provider，
        // 根据 driver 从 env 补值（driver-specific / LLM_API_KEY / OPENAI_API_KEY）。
        resolve_provider_api_keys(&mut config);

        let snapshot = ConfigSnapshot::new(config.clone());

        let mut writer = self.inner.write().await;
        writer.config = config.clone();
        drop(writer);

        self.tx.send_replace(snapshot);
        Ok(config)
    }

    /// Reload from all sources.
    pub async fn reload(&self) -> Result<(), String> {
        self.load().await?;
        Ok(())
    }

    /// Update config with a closure, persist, push snapshot.
    pub async fn update<F>(&self, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut Config),
    {
        let mut writer = self.inner.write().await;
        f(&mut writer.config);
        let config = writer.config.clone();

        if let Some(parent) = writer.global_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("创建配置目录失败: {e}"))?;
        }
        let content =
            serde_json::to_string_pretty(&config).map_err(|e| format!("序列化配置失败: {e}"))?;
        tokio::fs::write(&writer.global_path, content)
            .await
            .map_err(|e| format!("写入配置失败: {e}"))?;

        drop(writer);

        let snapshot = ConfigSnapshot::new(config);
        self.tx.send_replace(snapshot);
        Ok(())
    }

    /// Get current config (backward compat).
    pub async fn get(&self) -> Config {
        self.inner.read().await.config.clone()
    }
}

/// 对 `config.models.providers` 中 `api_key` 为空的 provider，
/// 根据 `driver` 从 env 补值（driver-specific → LLM_API_KEY → OPENAI_API_KEY）。
fn resolve_provider_api_keys(config: &mut Config) {
    for provider in config.models.providers.values_mut() {
        if !provider.api_key.is_empty() {
            continue;
        }
        // driver → driver-specific env
        if let Some(env_name) =
            share::config::domain::driver_env::driver_api_key_env_name(&provider.driver)
        {
            if let Ok(val) = std::env::var(env_name) {
                provider.api_key = val;
                continue;
            }
        }
        // fallback: LLM_API_KEY → OPENAI_API_KEY
        if let Ok(val) = std::env::var("LLM_API_KEY") {
            provider.api_key = val;
        } else if let Ok(val) = std::env::var("OPENAI_API_KEY") {
            provider.api_key = val;
        }
    }
}

#[async_trait]
impl ConfigReader for ConfigAppService {
    async fn snapshot(&self) -> ConfigSnapshot {
        self.tx.borrow().clone()
    }

    async fn watch(&self) -> watch::Receiver<ConfigSnapshot> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("aemeath_test_{name}_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// Create a ConfigAppService that writes to a temp file, NOT ~/.agents/aemeath.json
    fn test_svc(dir: &std::path::Path) -> ConfigAppService {
        let global = dir.join("aemeath.json");
        ConfigAppService::with_global_path(Some(dir), global)
    }

    #[tokio::test]
    async fn test_default_load() {
        let dir = test_dir("default_load");
        let svc = test_svc(&dir);
        let config = svc.load().await.unwrap();
        assert_eq!(config.model.context_size, 0);
    }

    #[tokio::test]
    async fn test_watch_and_update() {
        let dir = test_dir("watch_update");
        let svc = test_svc(&dir);
        let rx = svc.watch().await;

        svc.update(|c| c.model.name = "watch-test".into())
            .await
            .unwrap();

        assert!(rx.has_changed().unwrap_or(true));
        assert_eq!(rx.borrow().model_name(), "watch-test");
    }

    #[tokio::test]
    async fn test_cli_patch_overrides() {
        let dir = test_dir("cli_patch");
        let svc = test_svc(&dir);

        let mut cli = ConfigPatch::default();
        cli.model.get_or_insert(Default::default()).max_tokens = Some(99999);
        svc.set_cli_patch(cli).await;

        svc.load().await.unwrap();
        assert_eq!(svc.snapshot().await.max_tokens(), 99999);
    }
}

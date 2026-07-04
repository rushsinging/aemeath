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
        let global_path = share::config::paths::global_config_path();
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

        let config = chain.merge(Config::default());
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

    #[tokio::test]
    async fn test_default_load() {
        let tmp = std::env::temp_dir().join("aemeath_rt_test_default");
        let svc = ConfigAppService::new(Some(&tmp));
        let config = svc.load().await.unwrap();
        assert_eq!(config.model.context_size, 0);
    }

    #[tokio::test]
    async fn test_watch_and_update() {
        let tmp = std::env::temp_dir().join("aemeath_rt_test_watch");
        let svc = ConfigAppService::new(Some(&tmp));
        let mut rx = svc.watch().await;

        svc.update(|c| c.model.name = "watch-test".into())
            .await
            .unwrap();

        assert!(rx.has_changed().unwrap_or(true));
        assert_eq!(rx.borrow().model_name(), "watch-test");
    }

    #[tokio::test]
    async fn test_cli_patch_overrides() {
        let tmp = std::env::temp_dir().join("aemeath_rt_test_cli");
        let svc = ConfigAppService::new(Some(&tmp));

        let mut cli = ConfigPatch::default();
        cli.model.get_or_insert(Default::default()).max_tokens = Some(99999);
        svc.set_cli_patch(cli).await;

        svc.load().await.unwrap();
        assert_eq!(svc.snapshot().await.max_tokens(), 99999);
    }
}

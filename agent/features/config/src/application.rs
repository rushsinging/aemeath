use crate::contract::*;
use async_trait::async_trait;
use share::config::adapters::env as env_adapter;
use share::config::domain::merge::{ConfigPatch, PriorityChain};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tokio::sync::{watch, RwLock as AsyncRwLock};

pub struct ConfigAppService {
    tx: watch::Sender<ConfigSnapshot>,
    inner: AsyncRwLock<Inner>,
    active_location: RwLock<Option<ProjectConfigLocation>>,
}

struct Inner {
    config: Config,
    global_path: PathBuf,
    project_path: Option<PathBuf>,
    claude_project_settings_path: Option<PathBuf>,
    cli_patch: ConfigPatch,
}

pub struct ConfigWiring {
    service: std::sync::Arc<ConfigAppService>,
}

impl ConfigWiring {
    pub fn service(&self) -> std::sync::Arc<ConfigAppService> {
        self.service.clone()
    }

    pub fn reader(&self) -> std::sync::Arc<dyn ConfigReader> {
        self.service.clone()
    }

    pub fn query(&self) -> std::sync::Arc<dyn ConfigQuery> {
        self.service.clone()
    }

    pub fn writer(&self) -> std::sync::Arc<dyn ConfigWriter> {
        self.service.clone()
    }

    pub fn participant(&self) -> std::sync::Arc<dyn ProjectConfigParticipant> {
        self.service.clone()
    }
}

pub async fn wire_project_config(project_dir: &Path) -> Result<ConfigWiring, ConfigError> {
    let service = std::sync::Arc::new(ConfigAppService::new(Some(project_dir)));
    service.load().await.map_err(ConfigError::Load)?;
    Ok(ConfigWiring { service })
}

impl ConfigAppService {
    pub fn new(project_dir: Option<&Path>) -> Self {
        Self::with_global_path(project_dir, share::config::paths::global_config_path())
    }

    pub fn with_global_path(project_dir: Option<&Path>, global_path: PathBuf) -> Self {
        let project_path = project_dir.map(share::config::paths::project_config_path);
        let claude_project_settings_path =
            project_dir.map(share::config::paths::project_claude_settings_path);
        let initial = Config::default();
        let (tx, _) = watch::channel(ConfigSnapshot::new(initial.clone()));
        Self {
            tx,
            inner: AsyncRwLock::new(Inner {
                config: initial,
                global_path,
                project_path,
                claude_project_settings_path,
                cli_patch: ConfigPatch::default(),
            }),
            active_location: RwLock::new(None),
        }
    }

    pub async fn set_cli_patch(&self, patch: ConfigPatch) {
        self.inner.write().await.cli_patch = patch;
    }

    pub async fn load(&self) -> Result<(), String> {
        let inner = self.inner.read().await;
        let config = load_config(
            &inner.global_path,
            inner.project_path.as_deref(),
            inner.claude_project_settings_path.as_deref(),
            &inner.cli_patch,
        )
        .await;
        drop(inner);
        let snapshot = ConfigSnapshot::new(config.clone());
        self.inner.write().await.config = config;
        self.tx.send_replace(snapshot);
        Ok(())
    }

    async fn persist_and_publish(
        &self,
        config: Config,
    ) -> Result<ConfigSnapshot, ConfigPersistError> {
        let path = self.inner.read().await.global_path.clone();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(map_io_error)?;
        }
        let content =
            serde_json::to_string_pretty(&config).map_err(|_| ConfigPersistError::Serialization)?;
        tokio::fs::write(&path, content)
            .await
            .map_err(map_io_error)?;
        self.inner.write().await.config = config.clone();
        let snapshot = ConfigSnapshot::new(config);
        self.tx.send_replace(snapshot.clone());
        Ok(snapshot)
    }
}

async fn load_config(
    global_path: &Path,
    project_path: Option<&Path>,
    claude_project_settings_path: Option<&Path>,
    cli_patch: &ConfigPatch,
) -> Config {
    let mut chain = PriorityChain::new();
    for path in [
        claude_project_settings_path,
        Some(global_path),
        project_path,
    ]
    .into_iter()
    .flatten()
    {
        if let Ok(content) = tokio::fs::read_to_string(path).await {
            if let Ok(patch) = serde_json::from_str::<ConfigPatch>(&content) {
                chain.push(patch);
            }
        }
    }
    let env_patch = env_adapter::read();
    if !env_patch.is_empty() {
        chain.push(env_patch);
    }
    if !cli_patch.is_empty() {
        chain.push(cli_patch.clone());
    }
    let mut config = chain.merge(Config::default());
    resolve_provider_api_keys(&mut config);
    config
}

fn resolve_provider_api_keys(config: &mut Config) {
    for provider in config.models.providers.values_mut() {
        if !provider.api_key.is_empty() {
            continue;
        }
        if let Some(env_name) =
            share::config::domain::driver_env::driver_api_key_env_name(&provider.driver)
        {
            if let Ok(value) = std::env::var(env_name) {
                provider.api_key = value;
                continue;
            }
        }
        if let Ok(value) = std::env::var("LLM_API_KEY") {
            provider.api_key = value;
        } else if let Ok(value) = std::env::var("OPENAI_API_KEY") {
            provider.api_key = value;
        }
    }
}

fn map_io_error(error: std::io::Error) -> ConfigPersistError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        ConfigPersistError::PermissionDenied
    } else {
        ConfigPersistError::Io
    }
}

impl ConfigReader for ConfigAppService {
    fn committed_snapshot(&self) -> ConfigSnapshot {
        self.tx.borrow().clone()
    }

    fn subscribe_committed(&self) -> watch::Receiver<ConfigSnapshot> {
        self.tx.subscribe()
    }
}

#[async_trait]
impl ConfigQuery for ConfigAppService {
    async fn snapshot(&self) -> Result<ConfigSnapshot, ConfigQueryError> {
        Ok(self.committed_snapshot())
    }

    async fn subscribe(&self) -> Result<ConfigSubscription, ConfigQueryError> {
        let changes = self.subscribe_committed();
        let initial = changes.borrow().clone();
        Ok(ConfigSubscription { initial, changes })
    }
}

#[async_trait]
impl ConfigWriter for ConfigAppService {
    async fn update(&self, command: ConfigUpdate) -> Result<ConfigChangeSet, ConfigUpdateError> {
        let mut config = self.inner.read().await.config.clone();
        let field = match command {
            ConfigUpdate::SetModel { model } => {
                if model.trim().is_empty() {
                    return Err(ConfigUpdateError::Invalid("model 不能为空".into()));
                }
                config.models.default = model;
                ConfigField::Model
            }
            ConfigUpdate::SetPermissionMode { mode } => {
                config.permissions.mode = mode;
                ConfigField::PermissionMode
            }
            ConfigUpdate::SetMemoryConfig { config: memory } => {
                config.memory = memory;
                ConfigField::Memory
            }
        };
        let snapshot = self
            .persist_and_publish(config)
            .await
            .map_err(ConfigUpdateError::Persist)?;
        Ok(ConfigChangeSet {
            cause: ConfigChangeCause::ClientUpdate,
            fields: vec![field],
            snapshot,
        })
    }
}

#[async_trait]
impl ProjectConfigParticipant for ConfigAppService {
    async fn prepare_for_project(
        &self,
        location: &ProjectConfigLocation,
    ) -> Result<PreparedProjectConfig, ConfigError> {
        let inner = self.inner.read().await;
        let project_path = share::config::paths::project_config_path(location.search_root());
        let claude = share::config::paths::project_claude_settings_path(location.search_root());
        let config = load_config(
            &inner.global_path,
            Some(&project_path),
            Some(&claude),
            &inner.cli_patch,
        )
        .await;
        Ok(PreparedProjectConfig {
            location: location.clone(),
            snapshot: ConfigSnapshot::new(config),
        })
    }

    fn snapshot(&self) -> ConfigSnapshot {
        self.committed_snapshot()
    }

    fn commit_project(&self, prepared: PreparedProjectConfig) {
        *self.active_location.write().unwrap() = Some(prepared.location);
        self.tx.send_replace(prepared.snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn update_replaces_committed_snapshot_even_without_receiver() {
        let dir = tempfile::tempdir().unwrap();
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"));
        service
            .update(ConfigUpdate::SetModel {
                model: "provider/model".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            service.committed_snapshot().models().default,
            "provider/model"
        );
    }

    #[tokio::test]
    async fn subscription_initial_matches_committed_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"));
        let subscription = ConfigQuery::subscribe(&service).await.unwrap();
        assert_eq!(
            subscription.initial.model_name(),
            service.committed_snapshot().model_name()
        );
    }
}

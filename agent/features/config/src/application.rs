use crate::adapters::{
    encode_native_config, CompatibilityAdapter, ConfigAdapterError, ConfigValidator, EnvAdapter,
    EnvSource, FileAdapter, NativeConfigStore,
};
use crate::contract::*;
use async_trait::async_trait;
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
    native_store: Option<NativeConfigStore>,
    env_source: std::sync::Arc<dyn EnvSource>,
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

pub async fn wire_project_config_with_cli(
    project_dir: &Path,
    cli: crate::adapters::CliConfigInput,
) -> Result<ConfigWiring, ConfigError> {
    let service = std::sync::Arc::new(ConfigAppService::new(Some(project_dir)));
    service
        .set_cli_patch(crate::adapters::CliArgsAdapter::read(&cli))
        .await;
    service.load().await.map_err(ConfigError::Load)?;
    Ok(ConfigWiring { service })
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
            native_store: None,
            env_source: std::sync::Arc::new(crate::adapters::ProcessEnv),
        }
    }

    pub fn with_env_source(mut self, env_source: std::sync::Arc<dyn EnvSource>) -> Self {
        self.env_source = env_source;
        self
    }

    pub fn with_native_store(mut self, native_store: NativeConfigStore) -> Self {
        self.native_store = Some(native_store);
        self
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
            self.env_source.as_ref(),
        )
        .await
        .map_err(|error| format!("配置加载失败：{error:?}"))?;
        drop(inner);
        let snapshot = ConfigSnapshot::new(config.clone());
        self.inner.write().await.config = config;
        self.tx.send_replace(snapshot);
        Ok(())
    }
}

async fn load_config(
    global_path: &Path,
    project_path: Option<&Path>,
    claude_project_settings_path: Option<&Path>,
    cli_patch: &ConfigPatch,
    env_source: &dyn EnvSource,
) -> Result<Config, ConfigAdapterError> {
    let mut chain = PriorityChain::new();
    if let Some(patch) = FileAdapter::read(global_path).await? {
        chain.push(patch);
    }
    if let Some(path) = project_path {
        if let Some(patch) = FileAdapter::read(path).await? {
            chain.push(patch);
        }
    }
    if let Some(path) = claude_project_settings_path {
        if let Some(patch) = CompatibilityAdapter::read_one(path).await? {
            chain.push(patch);
        }
    }
    let env_patch = EnvAdapter::read(env_source);
    if !env_patch.is_empty() {
        chain.push(env_patch);
    }
    if !cli_patch.is_empty() {
        chain.push(cli_patch.clone());
    }
    let config = chain.merge(Config::default());
    ConfigValidator::validate(&config)?;
    Ok(config)
}

fn apply_update(
    config: &mut Config,
    command: ConfigUpdate,
) -> Result<ConfigField, ConfigUpdateError> {
    match command {
        ConfigUpdate::SetModel { model } => {
            if model.trim().is_empty() {
                return Err(ConfigUpdateError::Invalid("model 不能为空".into()));
            }
            config.models.default = model;
            Ok(ConfigField::Model)
        }
        ConfigUpdate::SetPermissionMode { mode } => {
            config.permissions.mode = mode;
            Ok(ConfigField::PermissionMode)
        }
        ConfigUpdate::SetMemoryConfig { config: memory } => {
            config.memory = memory;
            Ok(ConfigField::Memory)
        }
    }
}

fn map_commit_warning(warning: storage::api::CommitWarning) -> ConfigCommitWarning {
    match warning {
        storage::api::CommitWarning::PreviousPromotionPending => {
            ConfigCommitWarning::PreviousPromotionPending
        }
        storage::api::CommitWarning::JournalCleanupPending
        | storage::api::CommitWarning::MemberPublishRecoveryPending => {
            ConfigCommitWarning::JournalCleanupPending
        }
    }
}

fn map_adapter_persist_error(error: ConfigAdapterError) -> ConfigPersistError {
    match error {
        ConfigAdapterError::PermissionDenied => ConfigPersistError::PermissionDenied,
        ConfigAdapterError::UnsupportedDurability => ConfigPersistError::UnsupportedDurability,
        ConfigAdapterError::CorruptTransaction => ConfigPersistError::CorruptTransaction,
        ConfigAdapterError::Parse => ConfigPersistError::Serialization,
        ConfigAdapterError::Io | ConfigAdapterError::Invalid => ConfigPersistError::Io,
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
        let prepared = ProjectConfigParticipant::prepare_update(self, command).await?;
        match ProjectConfigParticipant::persist_update(self, prepared).await {
            ConfigPersistOutcome::NotCommitted(error) => Err(ConfigUpdateError::Persist(error)),
            ConfigPersistOutcome::Committed(ready) => {
                Ok(ProjectConfigParticipant::commit_update(self, ready))
            }
        }
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
            self.env_source.as_ref(),
        )
        .await
        .map_err(|error| ConfigError::Load(format!("配置加载失败：{error:?}")))?;
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

    async fn prepare_update(
        &self,
        command: ConfigUpdate,
    ) -> Result<PreparedConfigUpdate, ConfigUpdateError> {
        let mut config = self.inner.read().await.config.clone();
        let field = apply_update(&mut config, command)?;
        ConfigValidator::validate(&config)
            .map_err(|error| ConfigUpdateError::Invalid(format!("{error:?}")))?;
        let bytes = encode_native_config(&config)
            .map_err(|_| ConfigUpdateError::Persist(ConfigPersistError::Serialization))?;
        let project_key = self
            .active_location
            .read()
            .unwrap()
            .as_ref()
            .map(|location| location.key().to_string())
            .unwrap_or_else(|| "global".to_string());
        Ok(PreparedConfigUpdate {
            project_key,
            snapshot: ConfigSnapshot::new(config),
            bytes,
            fields: vec![field],
        })
    }

    async fn persist_update(&self, prepared: PreparedConfigUpdate) -> ConfigPersistOutcome {
        let Some(store) = &self.native_store else {
            return ConfigPersistOutcome::NotCommitted(ConfigPersistError::UnsupportedDurability);
        };
        match store
            .write_override(&prepared.project_key, &prepared.bytes)
            .await
        {
            Ok(warning) => ConfigPersistOutcome::Committed(ReadyConfigCommit {
                snapshot: prepared.snapshot,
                fields: prepared.fields,
                warning: warning.map(map_commit_warning),
            }),
            Err(error) => ConfigPersistOutcome::NotCommitted(map_adapter_persist_error(error)),
        }
    }

    fn commit_update(&self, ready: ReadyConfigCommit) -> ConfigChangeSet {
        let snapshot = ready.snapshot.clone();
        self.tx.send_replace(snapshot.clone());
        ConfigChangeSet {
            cause: ConfigChangeCause::ClientUpdate,
            fields: ready.fields,
            snapshot,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeEnv(std::collections::HashMap<String, String>);

    impl EnvSource for FakeEnv {
        fn get(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
    }

    #[tokio::test]
    async fn cli_layer_overrides_env_and_env_overrides_file() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("config.json");
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let store = NativeConfigStore::new(storage);
        let mut file_config = Config::default();
        file_config.model.name = "file-model".into();
        store
            .write_override("global", &encode_native_config(&file_config).unwrap())
            .await
            .unwrap();
        let service = ConfigAppService::with_global_path(Some(dir.path()), global)
            .with_native_store(store)
            .with_env_source(std::sync::Arc::new(FakeEnv(
                std::collections::HashMap::from([("AEMEATH_MODEL".into(), "env-model".into())]),
            )));
        service
            .set_cli_patch(crate::CliArgsAdapter::read(&crate::CliConfigInput {
                api_key: Some("cli-key".into()),
                model: Some("cli-model".into()),
                ..Default::default()
            }))
            .await;
        service.load().await.unwrap();
        assert_eq!(service.committed_snapshot().model_name(), "cli-model");
        assert_eq!(service.committed_snapshot().api_key(), Some("cli-key"));
    }

    #[tokio::test]
    async fn update_replaces_committed_snapshot_even_without_receiver() {
        let dir = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage));
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
    async fn prepare_update_does_not_publish_before_commit() {
        let dir = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage));
        let before = service.committed_snapshot().models().default.clone();
        let prepared = service
            .prepare_update(ConfigUpdate::SetModel {
                model: "local/model".into(),
            })
            .await
            .unwrap();
        assert_eq!(service.committed_snapshot().models().default, before);
        let ready = match service.persist_update(prepared).await {
            ConfigPersistOutcome::Committed(ready) => ready,
            ConfigPersistOutcome::NotCommitted(error) => panic!("unexpected {error:?}"),
        };
        service.commit_update(ready);
        assert_eq!(service.committed_snapshot().models().default, "local/model");
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

use crate::adapters::{
    config_fingerprint, encode_native_patch, merge_native_patches, source_fingerprints,
    CompatibilityAdapter, ConfigAdapterError, ConfigValidator, EnvAdapter, EnvSource, FileAdapter,
    NativeConfigStore, SourceFingerprints,
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
    active: RwLock<ActiveConfig>,
    source_fingerprints: RwLock<SourceFingerprints>,
    mutation_lock: tokio::sync::Mutex<()>,
    native_store: Option<NativeConfigStore>,
    env_source: std::sync::Arc<dyn EnvSource>,
}

struct ActiveConfig {
    config: Config,
    location: Option<ProjectConfigLocation>,
}

struct Inner {
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
    native_store: NativeConfigStore,
    cli: crate::adapters::CliConfigInput,
) -> Result<ConfigWiring, ConfigError> {
    log::debug!(
        target: crate::LOG_TARGET,
        "wire_project_config_with_cli: enter"
    );
    let result = async {
        let service =
            std::sync::Arc::new(ConfigAppService::for_project(project_dir, native_store)?);
        service
            .set_cli_patch(crate::adapters::CliArgsAdapter::read(&cli))
            .await;
        service.load().await.map_err(ConfigError::Load)?;
        Ok(ConfigWiring { service })
    }
    .await;
    match &result {
        Ok(_) => log::info!(
            target: crate::LOG_TARGET,
            "wire_project_config_with_cli: success"
        ),
        Err(_) => log::warn!(
            target: crate::LOG_TARGET,
            "wire_project_config_with_cli: failure"
        ),
    }
    result
}

pub async fn wire_project_config(
    project_dir: &Path,
    native_store: NativeConfigStore,
) -> Result<ConfigWiring, ConfigError> {
    let service = std::sync::Arc::new(ConfigAppService::for_project(project_dir, native_store)?);
    service.load().await.map_err(ConfigError::Load)?;
    Ok(ConfigWiring { service })
}

impl ConfigAppService {
    fn for_project(
        project_dir: &Path,
        native_store: NativeConfigStore,
    ) -> Result<Self, ConfigError> {
        let canonical = project_dir
            .canonicalize()
            .map_err(|_| ConfigError::InvalidLocation(ProjectConfigLocationError::NotCanonical))?;
        let location = ProjectConfigLocation::try_from_project_identity(
            canonical.clone(),
            canonical.to_string_lossy().as_bytes(),
        )
        .map_err(ConfigError::InvalidLocation)?;
        let service = Self::with_global_path(
            Some(project_dir),
            share::config::paths::global_config_path(),
        )
        .with_native_store(native_store);
        service.active.write().unwrap().location = Some(location);
        Ok(service)
    }

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
                global_path,
                project_path,
                claude_project_settings_path,
                cli_patch: ConfigPatch::default(),
            }),
            active: RwLock::new(ActiveConfig {
                config: initial,
                location: None,
            }),
            source_fingerprints: RwLock::new(SourceFingerprints::default()),
            mutation_lock: tokio::sync::Mutex::new(()),
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
        let project_key = self
            .active
            .read()
            .unwrap()
            .location
            .as_ref()
            .map(|location| location.key().to_string())
            .unwrap_or_else(|| "global".to_string());
        let config = load_config(
            &inner.global_path,
            inner.project_path.as_deref(),
            inner.claude_project_settings_path.as_deref(),
            &inner.cli_patch,
            self.native_store.as_ref(),
            &project_key,
            self.env_source.as_ref(),
        )
        .await
        .map_err(|error| format!("配置加载失败：{error:?}"))?;
        let global_path = inner.global_path.clone();
        let claude_path = inner.claude_project_settings_path.clone();
        let project_path = inner.project_path.clone();
        drop(inner);
        let snapshot = ConfigSnapshot::new(config.clone());
        self.active.write().unwrap().config = config;
        *self.source_fingerprints.write().unwrap() = source_fingerprints(
            &global_path,
            claude_path.as_deref(),
            project_path.as_deref(),
        )
        .await;
        self.tx.send_replace(snapshot);
        Ok(())
    }
}

async fn load_config(
    global_path: &Path,
    project_path: Option<&Path>,
    claude_project_settings_path: Option<&Path>,
    cli_patch: &ConfigPatch,
    native_store: Option<&NativeConfigStore>,
    project_key: &str,
    env_source: &dyn EnvSource,
) -> Result<Config, ConfigAdapterError> {
    let mut chain = PriorityChain::new();
    if let Some(patch) = FileAdapter::read(global_path).await? {
        chain.push(patch);
    }
    if let Some(path) = claude_project_settings_path {
        if let Some(patch) = CompatibilityAdapter::read_one(path).await? {
            chain.push(patch);
        }
    }
    if let Some(path) = project_path {
        if let Some(patch) = FileAdapter::read(path).await? {
            chain.push(patch);
        }
    }
    if let Some(store) = native_store {
        if let Some(patch) = store.read_override(project_key).await? {
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

fn patch_for_update(
    command: ConfigUpdate,
) -> Result<(ConfigField, ConfigPatch), ConfigUpdateError> {
    match command {
        ConfigUpdate::SetModel { model } => {
            if model.trim().is_empty() {
                return Err(ConfigUpdateError::Invalid("model 不能为空".into()));
            }
            Ok((
                ConfigField::Model,
                ConfigPatch {
                    model: Some(share::config::domain::merge::ModelConfigPatch {
                        name: Some(model.clone()),
                        ..Default::default()
                    }),
                    models: Some(share::config::domain::merge::ModelsConfigPatch {
                        default: Some(model),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ))
        }
        ConfigUpdate::SetPermissionMode { mode } => Ok((
            ConfigField::PermissionMode,
            ConfigPatch {
                permissions: Some(share::config::domain::merge::PermissionConfigPatch {
                    mode: Some(mode),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )),
        ConfigUpdate::SetMemoryConfig { config } => Ok((
            ConfigField::Memory,
            ConfigPatch {
                memory: Some(share::config::domain::merge::MemoryConfigPatch {
                    enabled: Some(config.enabled),
                    max_entries: Some(config.max_entries),
                    similarity_threshold: Some(config.similarity_threshold),
                    inject_count: Some(config.inject_count),
                    reflection: Some(share::config::domain::merge::ReflectionConfigPatch {
                        enabled: Some(config.reflection.enabled),
                        interval_turns: Some(config.reflection.interval_turns),
                        auto_apply_suggestions: Some(config.reflection.auto_apply_suggestions),
                        clear_model: config.reflection.model.is_none(),
                        model: config.reflection.model,
                    }),
                }),
                ..Default::default()
            },
        )),
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

#[async_trait]
impl ConfigReader for ConfigAppService {
    fn committed_snapshot(&self) -> ConfigSnapshot {
        self.tx.borrow().clone()
    }

    fn subscribe_committed(&self) -> watch::Receiver<ConfigSnapshot> {
        self.tx.subscribe()
    }

    async fn refresh_if_sources_changed(&self) -> ConfigRefreshOutcome {
        let _mutation = self.mutation_lock.lock().await;
        let inner = self.inner.read().await;
        let current_sources = source_fingerprints(
            &inner.global_path,
            inner.claude_project_settings_path.as_deref(),
            inner.project_path.as_deref(),
        )
        .await;
        if current_sources == *self.source_fingerprints.read().unwrap() {
            return ConfigRefreshOutcome::Unchanged;
        }

        let project_key = self
            .active
            .read()
            .unwrap()
            .location
            .as_ref()
            .map(|location| location.key().to_owned())
            .unwrap_or_else(|| "global".to_owned());
        let loaded = load_config(
            &inner.global_path,
            inner.project_path.as_deref(),
            inner.claude_project_settings_path.as_deref(),
            &inner.cli_patch,
            self.native_store.as_ref(),
            &project_key,
            self.env_source.as_ref(),
        )
        .await;
        drop(inner);

        let config = match loaded {
            Ok(config) => config,
            Err(error) => {
                return ConfigRefreshOutcome::Rejected {
                    error: refresh_error(error),
                }
            }
        };
        let candidate_fingerprint = match config_fingerprint(&config) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                return ConfigRefreshOutcome::Rejected {
                    error: refresh_error(error),
                }
            }
        };
        let active_fingerprint = match config_fingerprint(&self.active.read().unwrap().config) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                return ConfigRefreshOutcome::Rejected {
                    error: refresh_error(error),
                }
            }
        };
        *self.source_fingerprints.write().unwrap() = current_sources;
        if candidate_fingerprint == active_fingerprint {
            return ConfigRefreshOutcome::Unchanged;
        }

        let revision = self.committed_snapshot().revision().next();
        let snapshot = ConfigSnapshot::new_with_revision(revision, config.clone());
        self.active.write().unwrap().config = config;
        self.tx.send_replace(snapshot.clone());
        ConfigRefreshOutcome::Reloaded { snapshot }
    }
}

fn refresh_error(error: ConfigAdapterError) -> ConfigRefreshError {
    match error {
        ConfigAdapterError::Parse => ConfigRefreshError::Parse,
        ConfigAdapterError::Invalid => ConfigRefreshError::Invalid,
        ConfigAdapterError::Io
        | ConfigAdapterError::PermissionDenied
        | ConfigAdapterError::UnsupportedDurability
        | ConfigAdapterError::CorruptTransaction => ConfigRefreshError::Io,
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
        let _mutation = self.mutation_lock.lock().await;
        let prepared = ProjectConfigParticipant::prepare_update(self, command).await?;
        match ProjectConfigParticipant::persist_update(self, prepared).await {
            ConfigPersistOutcome::NotCommitted(error) => Err(ConfigUpdateError::Persist(error)),
            ConfigPersistOutcome::Committed(ready) => {
                Ok(ProjectConfigParticipant::commit_update(self, *ready))
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
            self.native_store.as_ref(),
            location.key(),
            self.env_source.as_ref(),
        )
        .await
        .map_err(|error| ConfigError::Load(format!("配置加载失败：{error:?}")))?;
        Ok(PreparedProjectConfig {
            location: location.clone(),
            config: config.clone(),
            snapshot: ConfigSnapshot::new(config),
        })
    }

    fn snapshot(&self) -> ConfigSnapshot {
        self.committed_snapshot()
    }

    async fn commit_project(&self, prepared: PreparedProjectConfig) {
        let _mutation = self.mutation_lock.lock().await;
        let revision = self.committed_snapshot().revision().next();
        let snapshot = prepared.snapshot.with_revision(revision);
        let mut active = self.active.write().unwrap();
        active.location = Some(prepared.location);
        active.config = prepared.config;
        drop(active);
        self.tx.send_replace(snapshot);
    }

    async fn prepare_update(
        &self,
        command: ConfigUpdate,
    ) -> Result<PreparedConfigUpdate, ConfigUpdateError> {
        let (base, project_key) = {
            let active = self.active.read().unwrap();
            (
                active.config.clone(),
                active
                    .location
                    .as_ref()
                    .map(|location| location.key().to_string())
                    .unwrap_or_else(|| "global".to_string()),
            )
        };
        let (field, override_patch) = patch_for_update(command)?;
        let config = share::config::domain::merge::apply_patch(base, override_patch.clone());
        let env_patch = EnvAdapter::read(self.env_source.as_ref());
        let config = share::config::domain::merge::apply_patch(config, env_patch);
        let cli_patch = self.inner.read().await.cli_patch.clone();
        let config = share::config::domain::merge::apply_patch(config, cli_patch);
        ConfigValidator::validate(&config)
            .map_err(|error| ConfigUpdateError::Invalid(format!("{error:?}")))?;
        let _ = encode_native_patch(&override_patch)
            .map_err(|_| ConfigUpdateError::Persist(ConfigPersistError::Serialization))?;
        Ok(PreparedConfigUpdate {
            project_key,
            config: config.clone(),
            override_patch,
            snapshot: ConfigSnapshot::new(config),
            fields: vec![field],
        })
    }

    async fn persist_update(&self, prepared: PreparedConfigUpdate) -> ConfigPersistOutcome {
        let Some(store) = &self.native_store else {
            return ConfigPersistOutcome::NotCommitted(ConfigPersistError::UnsupportedDurability);
        };
        let existing = match store.read_override(&prepared.project_key).await {
            Ok(existing) => existing.unwrap_or_default(),
            Err(error) => {
                return ConfigPersistOutcome::NotCommitted(map_adapter_persist_error(error))
            }
        };
        let override_patch = match merge_native_patches(existing, prepared.override_patch) {
            Ok(patch) => patch,
            Err(error) => {
                return ConfigPersistOutcome::NotCommitted(map_adapter_persist_error(error))
            }
        };
        let bytes = match encode_native_patch(&override_patch) {
            Ok(bytes) => bytes,
            Err(error) => {
                return ConfigPersistOutcome::NotCommitted(map_adapter_persist_error(error))
            }
        };
        match store.write_override(&prepared.project_key, &bytes).await {
            Ok(warning) => ConfigPersistOutcome::Committed(Box::new(ReadyConfigCommit {
                config: prepared.config,
                snapshot: prepared.snapshot,
                fields: prepared.fields,
                warning: warning.map(map_commit_warning),
            })),
            Err(error) => ConfigPersistOutcome::NotCommitted(map_adapter_persist_error(error)),
        }
    }

    fn commit_update(&self, ready: ReadyConfigCommit) -> ConfigChangeSet {
        let revision = self.committed_snapshot().revision().next();
        let snapshot = ready.snapshot.with_revision(revision);
        self.active.write().unwrap().config = ready.config;
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
    async fn cli_layer_overrides_env() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("config.json");
        let service = ConfigAppService::with_global_path(Some(dir.path()), global).with_env_source(
            std::sync::Arc::new(FakeEnv(std::collections::HashMap::from([(
                "AEMEATH_MODEL".into(),
                "env-model".into(),
            )]))),
        );
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
    async fn consecutive_updates_preserve_previously_committed_fields() {
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
        service
            .update(ConfigUpdate::SetPermissionMode {
                mode: share::config::PermissionModeConfig::AllowAll,
            })
            .await
            .unwrap();

        let snapshot = service.committed_snapshot();
        assert_eq!(snapshot.models().default, "provider/model");
        assert_eq!(snapshot.model_name(), "provider/model");
        assert_eq!(
            snapshot.permission_mode(),
            share::config::PermissionModeConfig::AllowAll
        );
        let rebuilt =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(std::sync::Arc::new(
                    storage::FileSystemBlobAdapter::new(dir.path()).unwrap(),
                )));
        rebuilt.load().await.unwrap();
        let snapshot = rebuilt.committed_snapshot();
        assert_eq!(snapshot.models().default, "provider/model");
        assert_eq!(
            snapshot.permission_mode(),
            share::config::PermissionModeConfig::AllowAll
        );
    }

    #[tokio::test]
    async fn concurrent_updates_are_serialized_without_losing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let service = std::sync::Arc::new(
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage)),
        );
        let model = {
            let service = service.clone();
            tokio::spawn(async move {
                service
                    .update(ConfigUpdate::SetModel {
                        model: "concurrent/model".into(),
                    })
                    .await
            })
        };
        let permission = {
            let service = service.clone();
            tokio::spawn(async move {
                service
                    .update(ConfigUpdate::SetPermissionMode {
                        mode: share::config::PermissionModeConfig::AllowAll,
                    })
                    .await
            })
        };
        model.await.unwrap().unwrap();
        permission.await.unwrap().unwrap();

        let snapshot = service.committed_snapshot();
        assert_eq!(snapshot.models().default, "concurrent/model");
        assert_eq!(
            snapshot.permission_mode(),
            share::config::PermissionModeConfig::AllowAll
        );
    }

    #[tokio::test]
    async fn runtime_override_is_restored_after_service_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("config.json");
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let store = NativeConfigStore::new(storage);
        let service = ConfigAppService::with_global_path(None, global.clone())
            .with_native_store(store.clone());
        service
            .update(ConfigUpdate::SetModel {
                model: "runtime/model".into(),
            })
            .await
            .unwrap();
        drop(service);

        let rebuilt = ConfigAppService::with_global_path(None, global).with_native_store(store);
        rebuilt.load().await.unwrap();

        assert_eq!(
            rebuilt.committed_snapshot().models().default,
            "runtime/model"
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
        service.commit_update(*ready);
        assert_eq!(service.committed_snapshot().models().default, "local/model");
    }

    #[tokio::test]
    async fn env_permission_override_remains_above_dynamic_local_update() {
        let dir = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage))
                .with_env_source(std::sync::Arc::new(FakeEnv(
                    std::collections::HashMap::from([(
                        "AEMEATH_PERMISSION_MODE".into(),
                        "allow_all".into(),
                    )]),
                )));
        service.load().await.unwrap();

        service
            .update(ConfigUpdate::SetPermissionMode {
                mode: share::config::PermissionModeConfig::Ask,
            })
            .await
            .unwrap();

        assert_eq!(
            service.committed_snapshot().permission_mode(),
            share::config::PermissionModeConfig::AllowAll
        );
    }

    #[tokio::test]
    async fn cli_permission_override_remains_highest_after_dynamic_update() {
        let dir = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage));
        service
            .set_cli_patch(crate::CliArgsAdapter::read(&crate::CliConfigInput {
                allow_all: true,
                ..Default::default()
            }))
            .await;
        service.load().await.unwrap();

        service
            .update(ConfigUpdate::SetPermissionMode {
                mode: share::config::PermissionModeConfig::Ask,
            })
            .await
            .unwrap();

        assert_eq!(
            service.committed_snapshot().permission_mode(),
            share::config::PermissionModeConfig::AllowAll
        );
    }

    #[tokio::test]
    async fn complete_priority_contract_uses_cli_over_env_over_local_over_global() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.json");
        std::fs::write(&global, r#"{"model":{"name":"global"}}"#).unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join(".agents")).unwrap();
        std::fs::write(
            project.join(".agents/aemeath.json"),
            r#"{"model":{"name":"project"}}"#,
        )
        .unwrap();
        let storage = std::sync::Arc::new(
            storage::FileSystemBlobAdapter::new(dir.path().join("storage")).unwrap(),
        );
        let store = NativeConfigStore::new(storage);
        let runtime = ConfigPatch {
            model: Some(share::config::domain::merge::ModelConfigPatch {
                name: Some("runtime".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        store
            .write_override("global", &encode_native_patch(&runtime).unwrap())
            .await
            .unwrap();
        let service = ConfigAppService::with_global_path(Some(&project), global)
            .with_native_store(store)
            .with_env_source(std::sync::Arc::new(FakeEnv(
                std::collections::HashMap::from([("AEMEATH_MODEL".into(), "env".into())]),
            )));
        service
            .set_cli_patch(crate::CliArgsAdapter::read(&crate::CliConfigInput {
                model: Some("cli".into()),
                ..Default::default()
            }))
            .await;

        service.load().await.unwrap();

        assert_eq!(service.committed_snapshot().model_name(), "cli");
    }

    #[tokio::test]
    async fn persist_failure_does_not_publish_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"));
        let before = service.committed_snapshot().models().default.clone();

        let error = service
            .update(ConfigUpdate::SetModel {
                model: "uncommitted/model".into(),
            })
            .await
            .unwrap_err();

        assert_eq!(
            error,
            ConfigUpdateError::Persist(ConfigPersistError::UnsupportedDurability)
        );
        assert_eq!(service.committed_snapshot().models().default, before);
    }

    #[tokio::test]
    async fn committed_update_notifies_subscription_with_same_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage));
        let mut subscription = ConfigQuery::subscribe(&service).await.unwrap();

        service
            .update(ConfigUpdate::SetModel {
                model: "notified/model".into(),
            })
            .await
            .unwrap();
        subscription.changes.changed().await.unwrap();

        assert_eq!(
            subscription.changes.borrow().models().default,
            "notified/model"
        );
        assert_eq!(
            subscription.changes.borrow().models().default,
            service.committed_snapshot().models().default
        );
    }

    #[tokio::test]
    async fn project_commit_becomes_baseline_for_following_update() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join(".agents")).unwrap();
        std::fs::write(
            project.join(".agents/aemeath.json"),
            r#"{"model":{"name":"project-model"}}"#,
        )
        .unwrap();
        let root = project.canonicalize().unwrap();
        let location =
            ProjectConfigLocation::try_from_project_identity(root, b"project-a").unwrap();
        let storage = std::sync::Arc::new(
            storage::FileSystemBlobAdapter::new(dir.path().join("storage")).unwrap(),
        );
        let service = ConfigAppService::with_global_path(None, dir.path().join("global.json"))
            .with_native_store(NativeConfigStore::new(storage));

        let prepared = service.prepare_for_project(&location).await.unwrap();
        service.commit_project(prepared).await;
        service
            .update(ConfigUpdate::SetPermissionMode {
                mode: share::config::PermissionModeConfig::AllowAll,
            })
            .await
            .unwrap();

        let snapshot = service.committed_snapshot();
        assert_eq!(snapshot.model_name(), "project-model");
        assert_eq!(
            snapshot.permission_mode(),
            share::config::PermissionModeConfig::AllowAll
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

    // ─────────────────────────────────────────────────────────────────
    // Logging contract for the real assembly entry
    // `wire_project_config_with_cli`.
    //
    // The boundary must:
    //   1. Emit a **debug** "enter" record on entry.
    //   2. Emit an **info/debug** "success" record when the Result is Ok.
    //   3. Emit a **warn** "failure" record when the Result is Err.
    //   4. Never leak sensitive config values (e.g. api_key) into any
    //      log message.
    //   5. Return the original error unchanged (behavior preserved).
    // ─────────────────────────────────────────────────────────────────

    thread_local! {
        static CAPTURED_CONFIG_LOGS: std::cell::RefCell<Vec<(log::Level, String)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    struct ConfigCapturingLogger;

    impl log::Log for ConfigCapturingLogger {
        fn enabled(&self, _metadata: &log::Metadata) -> bool {
            true
        }

        fn log(&self, record: &log::Record) {
            if record.target().starts_with("aemeath:agent:config") {
                CAPTURED_CONFIG_LOGS.with(|cell| {
                    cell.borrow_mut()
                        .push((record.level(), format!("{}", record.args())))
                });
            }
        }

        fn flush(&self) {}
    }

    /// Installs the capturing logger exactly once per test process. Safe to
    /// call from every test: `log::set_logger` succeeds only once; later
    /// calls are no-ops via `Once`. Capture storage is thread-local so
    /// parallel tests never observe each other's records.
    fn install_config_capturing_logger() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            log::set_boxed_logger(Box::new(ConfigCapturingLogger))
                .expect("capturing logger must install exactly once per process");
            log::set_max_level(log::LevelFilter::Trace);
        });
    }

    fn drain_captured_config_logs() -> Vec<(log::Level, String)> {
        CAPTURED_CONFIG_LOGS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
    }

    /// RAII guard that temporarily points `AEMEATH_AGENTS_DIR` at an
    /// isolated tempdir so the real assembly entry never touches the
    /// developer's `~/.agents` during tests.
    static AGENTS_DIR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct AgentsDirEnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        _dir: tempfile::TempDir,
    }

    fn test_native_store(root: &std::path::Path) -> NativeConfigStore {
        NativeConfigStore::new(std::sync::Arc::new(
            storage::FileSystemBlobAdapter::new(root.join("config-overrides"))
                .expect("create test config override blob"),
        ))
    }

    impl AgentsDirEnvGuard {
        fn new() -> Self {
            let lock = AGENTS_DIR_ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let dir = tempfile::tempdir().unwrap();
            unsafe {
                std::env::set_var("AEMEATH_AGENTS_DIR", dir.path());
            }
            Self {
                _lock: lock,
                _dir: dir,
            }
        }
    }

    impl Drop for AgentsDirEnvGuard {
        fn drop(&mut self) {
            unsafe {
                std::env::remove_var("AEMEATH_AGENTS_DIR");
            }
        }
    }

    #[tokio::test]
    async fn wire_entry_logs_debug_enter_then_success_on_ok() {
        install_config_capturing_logger();
        drain_captured_config_logs();
        let _env = AgentsDirEnvGuard::new();
        let project = tempfile::tempdir().unwrap();

        let result = wire_project_config_with_cli(
            project.path(),
            test_native_store(project.path()),
            crate::CliConfigInput::default(),
        )
        .await;

        assert!(
            result.is_ok(),
            "expected wire_project_config_with_cli to succeed"
        );
        let logs = drain_captured_config_logs();
        assert!(
            logs.iter()
                .any(|(l, m)| *l == log::Level::Debug && m.contains("enter")),
            "expected a debug 'enter' record; captured: {logs:?}"
        );
        assert!(
            logs.iter().any(|(l, m)| {
                (*l == log::Level::Info || *l == log::Level::Debug) && m.contains("success")
            }),
            "expected an info/debug 'success' record; captured: {logs:?}"
        );
        assert!(
            !logs.iter().any(|(l, _)| *l == log::Level::Warn),
            "no warn record expected on success; captured: {logs:?}"
        );
    }

    #[tokio::test]
    async fn wire_entry_logs_debug_enter_then_warn_failure_on_err_and_returns_original_error() {
        install_config_capturing_logger();
        drain_captured_config_logs();

        let missing_store_root = tempfile::tempdir().unwrap();
        let result = wire_project_config_with_cli(
            std::path::Path::new("/nonexistent/config/does/not/exist"),
            test_native_store(missing_store_root.path()),
            crate::CliConfigInput::default(),
        )
        .await;

        let error = match result {
            Ok(_) => panic!("expected failure for nonexistent project dir"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            ConfigError::InvalidLocation(ProjectConfigLocationError::NotCanonical),
            "original error must be returned unchanged"
        );
        let logs = drain_captured_config_logs();
        assert!(
            logs.iter()
                .any(|(l, m)| *l == log::Level::Debug && m.contains("enter")),
            "expected a debug 'enter' record; captured: {logs:?}"
        );
        assert!(
            logs.iter()
                .any(|(l, m)| *l == log::Level::Warn && m.contains("failure")),
            "expected a warn 'failure' record; captured: {logs:?}"
        );
        assert!(
            !logs.iter().any(|(_, m)| m.contains("success")),
            "no success record expected on failure; captured: {logs:?}"
        );
    }

    #[tokio::test]
    async fn wire_entry_never_logs_sensitive_config_values() {
        install_config_capturing_logger();
        drain_captured_config_logs();
        let _env = AgentsDirEnvGuard::new();
        let project = tempfile::tempdir().unwrap();
        let secret = "do-not-leak-this-api-key-42";

        let _ = wire_project_config_with_cli(
            project.path(),
            test_native_store(project.path()),
            crate::CliConfigInput {
                api_key: Some(secret.into()),
                ..Default::default()
            },
        )
        .await;

        let logs = drain_captured_config_logs();
        for (_, message) in &logs {
            assert!(
                !message.contains(secret),
                "sensitive api_key leaked into log message: {message}"
            );
        }
    }

    #[tokio::test]
    async fn project_aemeath_overrides_claude_compatibility() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agents")).unwrap();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(
            dir.path().join(".agents/aemeath.json"),
            r#"{"model":{"name":"aemeath"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".claude/settings.json"),
            r#"{"model":"claude"}"#,
        )
        .unwrap();
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("global.json"));
        service.load().await.unwrap();

        assert_eq!(service.committed_snapshot().model_name(), "aemeath");
    }

    #[tokio::test]
    async fn refresh_rejects_invalid_source_and_preserves_committed_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.json");
        std::fs::write(&global, r#"{"model":{"name":"first"}}"#).unwrap();
        let service = ConfigAppService::with_global_path(None, global.clone());
        service.load().await.unwrap();
        let before = service.committed_snapshot();

        std::fs::write(&global, "not json").unwrap();
        assert!(matches!(
            service.refresh_if_sources_changed().await,
            ConfigRefreshOutcome::Rejected {
                error: ConfigRefreshError::Parse
            }
        ));
        assert_eq!(service.committed_snapshot().model_name(), "first");
        assert_eq!(service.committed_snapshot().revision(), before.revision());
    }

    #[tokio::test]
    async fn refresh_does_not_publish_file_change_overridden_by_env() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.json");
        std::fs::write(&global, r#"{"model":{"name":"first"}}"#).unwrap();
        let service = ConfigAppService::with_global_path(None, global.clone()).with_env_source(
            std::sync::Arc::new(FakeEnv(std::collections::HashMap::from([(
                "AEMEATH_MODEL".into(),
                "env-model".into(),
            )]))),
        );
        service.load().await.unwrap();
        let before = service.committed_snapshot();

        std::fs::write(&global, r#"{"model":{"name":"second"}}"#).unwrap();
        assert!(matches!(
            service.refresh_if_sources_changed().await,
            ConfigRefreshOutcome::Unchanged
        ));
        assert_eq!(service.committed_snapshot().model_name(), "env-model");
        assert_eq!(service.committed_snapshot().revision(), before.revision());
    }

    #[tokio::test]
    async fn refresh_publishes_to_watch_subscribers_once() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("global.json");
        std::fs::write(&global, r#"{"model":{"name":"first"}}"#).unwrap();
        let service = ConfigAppService::with_global_path(None, global.clone());
        service.load().await.unwrap();
        let mut changes = service.subscribe_committed();

        std::fs::write(&global, r#"{"model":{"name":"second"}}"#).unwrap();
        assert!(matches!(
            service.refresh_if_sources_changed().await,
            ConfigRefreshOutcome::Reloaded { .. }
        ));
        changes.changed().await.unwrap();
        assert_eq!(changes.borrow().model_name(), "second");
    }
}

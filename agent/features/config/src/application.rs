use crate::adapters::{
    encode_native_patch, merge_native_patches, CompatibilityAdapter, ConfigAdapterError,
    ConfigValidator, EnvAdapter, EnvSource, FileAdapter, NativeConfigStore,
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
    cli: crate::adapters::CliConfigInput,
) -> Result<ConfigWiring, ConfigError> {
    let service = std::sync::Arc::new(ConfigAppService::for_project(project_dir)?);
    service
        .set_cli_patch(crate::adapters::CliArgsAdapter::read(&cli))
        .await;
    service.load().await.map_err(ConfigError::Load)?;
    Ok(ConfigWiring { service })
}

pub async fn wire_project_config(project_dir: &Path) -> Result<ConfigWiring, ConfigError> {
    let service = std::sync::Arc::new(ConfigAppService::for_project(project_dir)?);
    service.load().await.map_err(ConfigError::Load)?;
    Ok(ConfigWiring { service })
}

impl ConfigAppService {
    fn for_project(project_dir: &Path) -> Result<Self, ConfigError> {
        let canonical = project_dir
            .canonicalize()
            .map_err(|_| ConfigError::InvalidLocation(ProjectConfigLocationError::NotCanonical))?;
        let location = ProjectConfigLocation::try_from_project_identity(
            canonical.clone(),
            canonical.to_string_lossy().as_bytes(),
        )
        .map_err(ConfigError::InvalidLocation)?;
        let storage = storage::api::file_system_blob(
            share::config::paths::global_agents_dir().join("config-overrides"),
        )
        .map_err(|error| ConfigError::Load(format!("配置存储初始化失败：{error}")))?;
        let service = Self::with_global_path(
            Some(project_dir),
            share::config::paths::global_config_path(),
        )
        .with_native_store(NativeConfigStore::new(storage));
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
        drop(inner);
        let snapshot = ConfigSnapshot::new(config.clone());
        self.active.write().unwrap().config = config;
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
    if let Some(store) = native_store {
        if let Some(patch) = store.read_override(project_key).await? {
            chain.push(patch);
        }
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
        let mut active = self.active.write().unwrap();
        active.location = Some(prepared.location);
        active.config = prepared.config;
        drop(active);
        self.tx.send_replace(prepared.snapshot);
    }

    async fn prepare_update(
        &self,
        command: ConfigUpdate,
    ) -> Result<PreparedConfigUpdate, ConfigUpdateError> {
        let active = self.active.read().unwrap();
        let base = active.config.clone();
        let project_key = active
            .location
            .as_ref()
            .map(|location| location.key().to_string())
            .unwrap_or_else(|| "global".to_string());
        drop(active);
        let (field, override_patch) = patch_for_update(command)?;
        let config = share::config::domain::merge::apply_patch(base, override_patch.clone());
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
        let snapshot = ready.snapshot.clone();
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
    async fn complete_priority_contract_uses_runtime_override_last() {
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

        assert_eq!(service.committed_snapshot().model_name(), "runtime");
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
}

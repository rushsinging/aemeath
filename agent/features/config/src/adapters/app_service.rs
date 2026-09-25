//! 应用层服务：ConfigAppService 聚合配置加载、订阅发布与四个 port 的实现。
use crate::adapters::{
    config_fingerprint, encode_native_patch, merge_native_patches, source_fingerprints,
    CompatibilityAdapter, ConfigAdapterError, ConfigValidator, EnvAdapter, EnvSource, FileAdapter,
    NativeConfigStore, SourceFingerprints,
};
use crate::domain::{
    ConfigChangeCause, ConfigChangeSet, ConfigCommitWarning, ConfigError, ConfigField,
    ConfigPersistError, ConfigPersistOutcome, ConfigRefreshError, ConfigRefreshOutcome,
    ConfigSubscription, ConfigUpdate, ConfigUpdateError, PreparedConfigUpdate,
    PreparedProjectConfig, ProjectConfigLocation, ProjectConfigLocationError, ReadyConfigCommit,
};
use crate::ports::{ConfigReader, ConfigWriter, ProjectConfigParticipant};
use async_trait::async_trait;
use share::config::domain::merge::{ConfigPatch, PriorityChain};
use share::config::domain::scope::classify_application_scopes;
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

impl ConfigAppService {
    pub(crate) fn for_project(
        project_dir: &Path,
        native_store: NativeConfigStore,
    ) -> Result<Self, share::error::DomainError> {
        let canonical = project_dir.canonicalize().map_err(|_| {
            share::error::DomainError::from(ConfigError::InvalidLocation(
                ProjectConfigLocationError::NotCanonical,
            ))
        })?;
        let location = ProjectConfigLocation::try_from_project_identity(
            canonical.clone(),
            canonical.to_string_lossy().as_bytes(),
        )?;
        let service = Self::with_global_path(
            Some(project_dir),
            share::config::paths::global_config_path(),
        )
        .with_native_store(native_store);
        service.set_project_location(location);
        Ok(service)
    }

    /// 测试可指定独立 global 路径（context 集成测试的 Facade harness 依赖）；
    /// 生产构造一律经 crate 根 `wire_project_config*` 工厂（内部 `for_project`）。
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

    /// 测试专用：注入 FakeEnv；生产路径固定使用 ProcessEnv。
    #[cfg(test)]
    pub(crate) fn with_env_source(mut self, env_source: std::sync::Arc<dyn EnvSource>) -> Self {
        self.env_source = env_source;
        self
    }

    /// 测试 harness 注入自定义 store（context 集成测试）；生产构造经
    /// crate 根 `wire_project_config*` 工厂（内部 `for_project`，pub(crate)）。
    pub fn with_native_store(mut self, native_store: NativeConfigStore) -> Self {
        self.native_store = Some(native_store);
        self
    }

    pub(crate) fn set_project_location(&self, location: ProjectConfigLocation) {
        self.active.write().unwrap().location = Some(location);
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
        .map_err(|error| format!("配置加载失败：{error}"))?;
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
                    inject_token_budget: Some(config.inject_token_budget),
                    reflection: Some(share::config::domain::merge::ReflectionConfigPatch {
                        enabled: Some(config.reflection.enabled),
                        interval_runs: Some(config.reflection.interval_runs),
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

fn map_commit_warning(warning: storage::CommitWarning) -> ConfigCommitWarning {
    match warning {
        storage::CommitWarning::PreviousPromotionPending => {
            ConfigCommitWarning::PreviousPromotionPending
        }
        storage::CommitWarning::JournalCleanupPending
        | storage::CommitWarning::MemberPublishRecoveryPending => {
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
        ConfigAdapterError::Io
        | ConfigAdapterError::Invalid
        | ConfigAdapterError::InvalidModel { .. } => ConfigPersistError::Io,
    }
}

#[async_trait]
impl ConfigReader for ConfigAppService {
    async fn snapshot(&self) -> Result<ConfigSnapshot, share::error::DomainError> {
        Ok(self.committed_snapshot())
    }

    async fn subscribe(&self) -> Result<ConfigSubscription, share::error::DomainError> {
        let changes = self.subscribe_committed();
        let initial = changes.borrow().clone();
        Ok(ConfigSubscription { initial, changes })
    }

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
        let active = self.active.read().unwrap();
        let active_fingerprint = match config_fingerprint(&active.config) {
            Ok(fingerprint) => fingerprint,
            Err(error) => {
                return ConfigRefreshOutcome::Rejected {
                    error: refresh_error(error),
                }
            }
        };
        let scopes = classify_application_scopes(&active.config, &config);
        drop(active);
        *self.source_fingerprints.write().unwrap() = current_sources;
        if candidate_fingerprint == active_fingerprint {
            return ConfigRefreshOutcome::Unchanged;
        }

        let revision = self.committed_snapshot().revision().next();
        let snapshot = ConfigSnapshot::new_with_revision(revision, config.clone());
        self.active.write().unwrap().config = config;
        self.tx.send_replace(snapshot.clone());
        ConfigRefreshOutcome::Reloaded { snapshot, scopes }
    }
}

fn refresh_error(error: ConfigAdapterError) -> ConfigRefreshError {
    match error {
        ConfigAdapterError::Parse => ConfigRefreshError::Parse,
        ConfigAdapterError::Invalid | ConfigAdapterError::InvalidModel { .. } => {
            ConfigRefreshError::Invalid
        }
        ConfigAdapterError::Io
        | ConfigAdapterError::PermissionDenied
        | ConfigAdapterError::UnsupportedDurability
        | ConfigAdapterError::CorruptTransaction => ConfigRefreshError::Io,
    }
}

#[async_trait]
#[async_trait]
impl ConfigWriter for ConfigAppService {
    async fn update(
        &self,
        command: ConfigUpdate,
    ) -> Result<ConfigChangeSet, share::error::DomainError> {
        let _mutation = self.mutation_lock.lock().await;
        let prepared = ProjectConfigParticipant::prepare_update(self, command).await?;
        match ProjectConfigParticipant::persist_update(self, prepared).await {
            ConfigPersistOutcome::NotCommitted(error) => Err(share::error::DomainError::from(
                ConfigUpdateError::Persist(error),
            )),
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
    ) -> Result<PreparedProjectConfig, share::error::DomainError> {
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
    ) -> Result<PreparedConfigUpdate, share::error::DomainError> {
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

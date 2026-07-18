use crate::adapters::{
    encode_native_config, CompatibilityAdapter, ConfigAdapterError, ConfigValidator, EnvAdapter,
    FileAdapter, NativeConfigStore, ProcessEnv,
};
use crate::contract::*;
use async_trait::async_trait;
use share::config::domain::merge::{ConfigPatch, PriorityChain};
use share::config::domain::snapshot::{ConfigRevision, ConfigSnapshot};
use share::config::Config;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::sync::watch;

/// Constructs the production [`NativeConfigStore`] backed by the global agents
/// directory via [`storage::FileSystemBlobAdapter`].
///
/// The physical root is `~/.agents` (or `$AEMEATH_AGENTS_DIR`). This follows
/// the same pattern as other composition-level adapters (dataset, blob). The
/// `ConfigAppService` itself never touches the filesystem — only this factory
/// function and the injected `AtomicBlobPort` do.
fn create_production_native_store() -> Result<NativeConfigStore, String> {
    let adapter = storage::FileSystemBlobAdapter::new(share::config::paths::global_agents_dir())
        .map_err(|e| format!("配置存储初始化失败：{e}"))?;
    Ok(NativeConfigStore::new(Arc::new(adapter)))
}

pub struct ConfigAppService {
    tx: watch::Sender<ConfigSnapshot>,
    inner: RwLock<Inner>,
    active_location: RwLock<Option<ProjectConfigLocation>>,
    native_store: Option<NativeConfigStore>,
}

struct Inner {
    config: Config,
    revision: ConfigRevision,
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

    pub fn participant(&self) -> std::sync::Arc<dyn ProjectConfigParticipant> {
        self.service.clone()
    }
}

pub async fn wire_project_config_with_cli(
    project_dir: &Path,
    cli: crate::adapters::CliConfigInput,
) -> Result<ConfigWiring, ConfigError> {
    let native_store = create_production_native_store().map_err(ConfigError::Load)?;
    let service = std::sync::Arc::new(
        ConfigAppService::new(Some(project_dir)).with_native_store(native_store),
    );
    service
        .set_cli_patch(crate::adapters::CliArgsAdapter::read(&cli))
        .await;
    service.load().await.map_err(ConfigError::Load)?;
    Ok(ConfigWiring { service })
}

pub async fn wire_project_config(project_dir: &Path) -> Result<ConfigWiring, ConfigError> {
    let native_store = create_production_native_store().map_err(ConfigError::Load)?;
    let service = std::sync::Arc::new(
        ConfigAppService::new(Some(project_dir)).with_native_store(native_store),
    );
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
            inner: RwLock::new(Inner {
                config: initial,
                revision: ConfigRevision::default(),
                global_path,
                project_path,
                claude_project_settings_path,
                cli_patch: ConfigPatch::default(),
            }),
            active_location: RwLock::new(None),
            native_store: None,
        }
    }

    pub fn with_native_store(mut self, native_store: NativeConfigStore) -> Self {
        self.native_store = Some(native_store);
        self
    }

    pub async fn set_cli_patch(&self, patch: ConfigPatch) {
        self.inner.write().unwrap().cli_patch = patch;
    }

    pub async fn load(&self) -> Result<(), String> {
        let (global_path, project_path, claude_path, cli_patch) = {
            let inner = self.inner.read().unwrap();
            (
                inner.global_path.clone(),
                inner.project_path.clone(),
                inner.claude_project_settings_path.clone(),
                inner.cli_patch.clone(),
            )
        };
        let config = load_config(
            &global_path,
            project_path.as_deref(),
            claude_path.as_deref(),
            &cli_patch,
        )
        .await
        .map_err(|error| format!("配置加载失败：{error:?}"))?;
        let snapshot = {
            let mut inner = self.inner.write().unwrap();
            inner.revision = inner.revision.next();
            inner.config = config.clone();
            ConfigSnapshot::new_with_revision(inner.revision, config)
        };
        self.tx.send_replace(snapshot);
        Ok(())
    }

    /// Replays the full config chain (File → Compatibility → Env → CLI) from
    /// `Config::default()` for the given project `location`, then reads the
    /// durable override from [`NativeConfigStore`] as the **last** layer.
    ///
    /// This is the core "chain replay + override" function used by both
    /// `prepare_for_project` and `prepare_update`. It guarantees that:
    ///
    /// - The durable override always wins over File / Env / CLI (it is pushed
    ///   last in the `PriorityChain`).
    /// - The same chain is replayed on every call, so process restart or
    ///   file changes do not silently drop the override.
    /// - If no override exists, the result is the plain chain merge.
    async fn load_config_with_override(
        &self,
        location: &ProjectConfigLocation,
    ) -> Result<Config, ConfigError> {
        let (global_path, cli_patch) = {
            let inner = self.inner.read().unwrap();
            (inner.global_path.clone(), inner.cli_patch.clone())
        };
        let project_path = share::config::paths::project_config_path(location.search_root());
        let claude = share::config::paths::project_claude_settings_path(location.search_root());

        let mut chain = PriorityChain::new();
        if let Some(patch) = FileAdapter::read(&global_path)
            .await
            .map_err(|e| ConfigError::Load(format!("配置加载失败：{e:?}")))?
        {
            chain.push(patch);
        }
        if let Some(patch) = FileAdapter::read(&project_path)
            .await
            .map_err(|e| ConfigError::Load(format!("配置加载失败：{e:?}")))?
        {
            chain.push(patch);
        }
        if let Some(patch) = CompatibilityAdapter::read_one(&claude)
            .await
            .map_err(|e| ConfigError::Load(format!("配置加载失败：{e:?}")))?
        {
            chain.push(patch);
        }
        let env_patch = EnvAdapter::read(&ProcessEnv);
        if !env_patch.is_empty() {
            chain.push(env_patch);
        }
        if !cli_patch.is_empty() {
            chain.push(cli_patch);
        }
        // Durable override is always the last (highest-priority) layer.
        if let Some(store) = &self.native_store {
            if let Some(override_patch) = store
                .read_override(location.key())
                .await
                .map_err(|e| ConfigError::Load(format!("override 读取失败：{e:?}")))?
            {
                chain.push(override_patch);
            }
        }
        let mut config = chain.merge(Config::default());
        resolve_provider_api_keys(&mut config);
        ConfigValidator::validate(&config)
            .map_err(|e| ConfigError::Load(format!("配置校验失败：{e:?}")))?;
        Ok(config)
    }
}

async fn load_config(
    global_path: &Path,
    project_path: Option<&Path>,
    claude_project_settings_path: Option<&Path>,
    cli_patch: &ConfigPatch,
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
    let env_patch = EnvAdapter::read(&ProcessEnv);
    if !env_patch.is_empty() {
        chain.push(env_patch);
    }
    if !cli_patch.is_empty() {
        chain.push(cli_patch.clone());
    }
    let mut config = chain.merge(Config::default());
    resolve_provider_api_keys(&mut config);
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

impl ConfigReader for ConfigAppService {
    fn committed_snapshot(&self) -> ConfigSnapshot {
        self.tx.borrow().clone()
    }

    fn subscribe_committed(&self) -> watch::Receiver<ConfigSnapshot> {
        self.tx.subscribe()
    }
}

#[async_trait]
impl ProjectConfigParticipant for ConfigAppService {
    async fn prepare_for_project(
        &self,
        location: &ProjectConfigLocation,
    ) -> Result<PreparedProjectConfig, ConfigError> {
        let config = self.load_config_with_override(location).await?;
        Ok(PreparedProjectConfig {
            location: location.clone(),
            snapshot: ConfigSnapshot::new(config),
        })
    }

    fn snapshot(&self) -> ConfigSnapshot {
        self.committed_snapshot()
    }

    fn commit_project(&self, prepared: PreparedProjectConfig) {
        let snapshot = {
            let mut inner = self.inner.write().unwrap();
            inner.revision = inner.revision.next();
            inner.config = prepared.snapshot.to_config();
            prepared.snapshot.with_revision(inner.revision)
        };
        *self.active_location.write().unwrap() = Some(prepared.location);
        self.tx.send_replace(snapshot);
    }

    async fn prepare_update(
        &self,
        command: ConfigUpdate,
    ) -> Result<PreparedConfigUpdate, ConfigUpdateError> {
        // Reject if there is no active project location — the "global" fallback
        // is removed to prevent cross-project config leakage.
        let location = self
            .active_location
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| {
                ConfigUpdateError::Invalid(
                    "没有活跃的项目配置位置，无法更新配置。请先通过 prepare_for_project + commit_project 建立项目上下文。".into(),
                )
            })?;
        // Replay the full chain (File → Compatibility → Env → CLI → durable
        // override) from Config::default(), then apply the command on top.
        // This ensures the update is based on the latest source files + override,
        // not a potentially stale in-memory snapshot.
        let mut config = self
            .load_config_with_override(&location)
            .await
            .map_err(|e| ConfigUpdateError::Invalid(format!("配置重放失败：{e:?}")))?;
        let field = apply_update(&mut config, command)?;
        ConfigValidator::validate(&config)
            .map_err(|error| ConfigUpdateError::Invalid(format!("{error:?}")))?;
        let bytes = encode_native_config(&config)
            .map_err(|_| ConfigUpdateError::Persist(ConfigPersistError::Serialization))?;
        Ok(PreparedConfigUpdate {
            project_key: location.key().to_string(),
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
        let snapshot = {
            let mut inner = self.inner.write().unwrap();
            inner.revision = inner.revision.next();
            inner.config = ready.snapshot.to_config();
            ready.snapshot.with_revision(inner.revision)
        };
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

    /// Test-only convenience: runs the full prepare → persist → commit pipeline
    /// via the [`ProjectConfigParticipant`] trait. Production code must go
    /// through the gate-aware [`config::ConfigWriter`] façade produced by
    /// `MainSessionWiring::config_writer()`.
    async fn participant_update(
        service: &ConfigAppService,
        command: ConfigUpdate,
    ) -> Result<ConfigChangeSet, ConfigUpdateError> {
        let prepared = ProjectConfigParticipant::prepare_update(service, command).await?;
        match ProjectConfigParticipant::persist_update(service, prepared).await {
            ConfigPersistOutcome::NotCommitted(error) => Err(ConfigUpdateError::Persist(error)),
            ConfigPersistOutcome::Committed(ready) => {
                Ok(ProjectConfigParticipant::commit_update(service, ready))
            }
        }
    }

    /// Test helper: establishes the active project location on the service
    /// by running prepare_for_project + commit_project. Returns the location
    /// for further assertions.
    async fn establish_active_location(
        service: &ConfigAppService,
        root: &Path,
    ) -> ProjectConfigLocation {
        let canonical = root.canonicalize().unwrap();
        let location =
            ProjectConfigLocation::try_from_project_identity(canonical, b"test-project").unwrap();
        let prepared = service.prepare_for_project(&location).await.unwrap();
        service.commit_project(prepared);
        location
    }

    #[tokio::test]
    async fn update_replaces_committed_snapshot_even_without_receiver() {
        let dir = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage));
        establish_active_location(&service, dir.path()).await;
        participant_update(
            &service,
            ConfigUpdate::SetModel {
                model: "provider/model".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            service.committed_snapshot().models().default,
            "provider/model"
        );
    }

    #[tokio::test]
    async fn consecutive_commits_advance_revision_and_preserve_previous_values() {
        let dir = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage));
        establish_active_location(&service, dir.path()).await;
        let initial = service.committed_snapshot().revision();

        participant_update(
            &service,
            ConfigUpdate::SetModel {
                model: "provider/first".into(),
            },
        )
        .await
        .unwrap();
        let first = service.committed_snapshot();
        participant_update(
            &service,
            ConfigUpdate::SetPermissionMode {
                mode: share::config::PermissionModeConfig::AllowAll,
            },
        )
        .await
        .unwrap();
        let second = service.committed_snapshot();

        assert_eq!(first.revision(), initial.next());
        assert_eq!(second.revision(), first.revision().next());
        assert_eq!(second.models().default, "provider/first");
        assert_eq!(
            second.permission_mode(),
            share::config::PermissionModeConfig::AllowAll
        );
    }

    #[tokio::test]
    async fn prepare_update_does_not_publish_before_commit() {
        let dir = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage));
        establish_active_location(&service, dir.path()).await;
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
        let changes = service.subscribe_committed();
        let initial = changes.borrow().clone();
        assert_eq!(
            initial.model_name(),
            service.committed_snapshot().model_name()
        );
    }

    #[tokio::test]
    async fn load_increments_revision_monotonically() {
        let dir = tempfile::tempdir().unwrap();
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"));
        let before = service.committed_snapshot().revision();
        service.load().await.unwrap();
        let after = service.committed_snapshot().revision();
        assert_eq!(after, before.next());
    }

    #[tokio::test]
    async fn watch_and_committed_snapshot_share_revision_after_update() {
        let dir = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage));
        establish_active_location(&service, dir.path()).await;
        let rx = service.subscribe_committed();
        participant_update(
            &service,
            ConfigUpdate::SetModel {
                model: "watch/model".into(),
            },
        )
        .await
        .unwrap();
        // The watch receiver sees the same revision as committed_snapshot().
        assert_eq!(
            rx.borrow().revision(),
            service.committed_snapshot().revision()
        );
    }

    #[tokio::test]
    async fn commit_project_increments_revision_and_updates_committed_config() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"));
        let before = service.committed_snapshot().revision();
        let location =
            ProjectConfigLocation::try_from_project_identity(root, b"test-project").unwrap();
        let prepared = service.prepare_for_project(&location).await.unwrap();
        service.commit_project(prepared);
        let after = service.committed_snapshot().revision();
        assert_eq!(after, before.next());
    }

    // ── New tests for CFG5: durable override as chain layer ──

    /// `prepare_update` rejects when no active location is set, instead of
    /// falling back to a "global" key.
    #[tokio::test]
    async fn prepare_update_rejects_without_active_location() {
        let dir = tempfile::tempdir().unwrap();
        let storage = std::sync::Arc::new(storage::FileSystemBlobAdapter::new(dir.path()).unwrap());
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage));
        // No establish_active_location — active_location is None.
        let result = service
            .prepare_update(ConfigUpdate::SetModel {
                model: "rejected/model".into(),
            })
            .await;
        assert!(
            matches!(result, Err(ConfigUpdateError::Invalid(_))),
            "expected Invalid error, got {result:?}"
        );
    }

    /// Durable override persists across a simulated process restart: after
    /// writing an override and creating a *fresh* service backed by the same
    /// store, `prepare_for_project` must replay the override as the last layer.
    #[tokio::test]
    async fn durable_override_survives_process_restart() {
        let dir = tempfile::tempdir().unwrap();
        let store_dir = tempfile::tempdir().unwrap();
        let storage =
            std::sync::Arc::new(storage::FileSystemBlobAdapter::new(store_dir.path()).unwrap());

        // First "process": set model via update.
        let service1 =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage.clone()));
        let location = establish_active_location(&service1, dir.path()).await;
        participant_update(
            &service1,
            ConfigUpdate::SetModel {
                model: "restart/model".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            service1.committed_snapshot().models().default,
            "restart/model"
        );

        // Second "process": fresh service, same store.
        let service2 =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage));
        let prepared = service2.prepare_for_project(&location).await.unwrap();
        // The override must be replayed — model reflects the persisted value.
        assert_eq!(
            prepared.snapshot().models().default,
            "restart/model",
            "durable override must survive process restart"
        );
    }

    /// Cross-project override isolation: writing an override for project A
    /// must NOT affect project B's config.
    #[tokio::test]
    async fn cross_project_override_isolation() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let store_dir = tempfile::tempdir().unwrap();
        let storage =
            std::sync::Arc::new(storage::FileSystemBlobAdapter::new(store_dir.path()).unwrap());

        // Project A: set model via update.
        let service = ConfigAppService::with_global_path(
            Some(dir_a.path()),
            dir_a.path().join("config.json"),
        )
        .with_native_store(NativeConfigStore::new(storage.clone()));
        let loc_a = establish_active_location(&service, dir_a.path()).await;
        participant_update(
            &service,
            ConfigUpdate::SetModel {
                model: "project-a/model".into(),
            },
        )
        .await
        .unwrap();

        // Project B: different location, same store.
        let canonical_b = dir_b.path().canonicalize().unwrap();
        let loc_b =
            ProjectConfigLocation::try_from_project_identity(canonical_b, b"project-b").unwrap();
        assert_ne!(
            loc_a.key(),
            loc_b.key(),
            "locations must have different keys"
        );

        let prepared_b = service.prepare_for_project(&loc_b).await.unwrap();
        // Project B should NOT see project A's override.
        assert_ne!(
            prepared_b.snapshot().models().default,
            "project-a/model",
            "cross-project override must NOT leak"
        );
    }

    /// Env / CLI cannot override the durable runtime override.
    ///
    /// After persisting an override that sets model to "override/model",
    /// even if the CLI patch sets a different model, `prepare_for_project`
    /// must return the override value because the override is the last layer.
    #[tokio::test]
    async fn env_cli_cannot_override_durable_override() {
        let dir = tempfile::tempdir().unwrap();
        let store_dir = tempfile::tempdir().unwrap();
        let storage =
            std::sync::Arc::new(storage::FileSystemBlobAdapter::new(store_dir.path()).unwrap());
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage));
        let location = establish_active_location(&service, dir.path()).await;

        // Set model via durable override.
        participant_update(
            &service,
            ConfigUpdate::SetModel {
                model: "override/model".into(),
            },
        )
        .await
        .unwrap();

        // Now set a CLI patch that tries to override the model.
        let cli_patch = crate::adapters::CliArgsAdapter::read(&crate::adapters::CliConfigInput {
            model: Some("cli/model".into()),
            ..Default::default()
        });
        service.set_cli_patch(cli_patch).await;

        // prepare_for_project must still return the durable override value.
        let prepared = service.prepare_for_project(&location).await.unwrap();
        assert_eq!(
            prepared.snapshot().models().default,
            "override/model",
            "durable override must win over CLI"
        );
    }

    /// `prepare_update` replays the chain from default rather than cloning
    /// the in-memory config. This ensures file changes are picked up.
    #[tokio::test]
    async fn prepare_update_replays_chain_not_clone_current() {
        let dir = tempfile::tempdir().unwrap();
        let store_dir = tempfile::tempdir().unwrap();
        let storage =
            std::sync::Arc::new(storage::FileSystemBlobAdapter::new(store_dir.path()).unwrap());
        let service =
            ConfigAppService::with_global_path(Some(dir.path()), dir.path().join("config.json"))
                .with_native_store(NativeConfigStore::new(storage));
        let _location = establish_active_location(&service, dir.path()).await;

        // Write a project config file with model "file/model".
        let agents_dir = dir.path().join(".agents");
        tokio::fs::create_dir_all(&agents_dir).await.unwrap();
        tokio::fs::write(
            agents_dir.join("aemeath.json"),
            r#"{"models":{"default":"file/model"}}"#,
        )
        .await
        .unwrap();

        // prepare_update should replay the chain and see "file/model" from the
        // file, then apply the command on top.
        let prepared = service
            .prepare_update(ConfigUpdate::SetModel {
                model: "updated/model".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            prepared.snapshot().models().default,
            "updated/model",
            "command should be applied on top of replayed chain"
        );

        // The persisted bytes should contain the full config with "updated/model".
        let config: Config = serde_json::from_slice(&prepared.bytes).unwrap();
        assert_eq!(config.models.default, "updated/model");
    }
}

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:config";
mod adapters;
mod domain;
mod ports;

pub use adapters::{
    CliArgsAdapter, CliConfigInput, ConfigAppService, EnvAdapter, FileAdapter, NativeConfigStore,
};
pub use domain::{
    ConfigChangeCause, ConfigChangeSet, ConfigCommitWarning, ConfigError, ConfigField,
    ConfigPersistError, ConfigPersistOutcome, ConfigQueryError, ConfigRefreshOutcome,
    ConfigSubscription, ConfigUpdate, ConfigUpdateError, PreparedConfigUpdate,
    PreparedProjectConfig, ProjectConfigLocation, ProjectConfigLocationError,
};
pub use ports::{ConfigQuery, ConfigReader, ConfigWriter, ProjectConfigParticipant};

// ---------- composition-only wiring（crate 根装配点，06-code-organization 基线形态） ----------
use std::path::Path;

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

/// Composition 的唯一 `NativeConfigStore` 构造入口：blob 由调用方
/// （composition）经 `storage::file_system_blob` 选定，config 不拥有
/// 文件系统实现选择权；`NativeConfigStore::new` 已收窄 `pub(crate)`，
/// crate 外散落构造在编译期不可达。
pub fn native_override_store(
    blob: std::sync::Arc<dyn storage::AtomicBlobPort>,
) -> NativeConfigStore {
    NativeConfigStore::new(blob)
}

pub async fn wire_project_config_with_cli(
    project_dir: &Path,
    native_store: NativeConfigStore,
    cli: CliConfigInput,
) -> Result<ConfigWiring, ConfigError> {
    log::debug!(
        target: crate::LOG_TARGET,
        "wire_project_config_with_cli: enter"
    );
    let result = async {
        let service =
            std::sync::Arc::new(ConfigAppService::for_project(project_dir, native_store)?);
        service.set_cli_patch(CliArgsAdapter::read(&cli)).await;
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

/// Like [`wire_project_config_with_cli`], but the global config path is
/// bounded to `agents_dir.join("aemeath.json")` instead of reading
/// `share::config::paths::global_config_path()` (which reads process env vars
/// via `AEMEATH_AGENTS_DIR`). Used by tests in `-p composition` to inject a
/// temp `agents/` directory tree without env mutation; production callers
/// pass `share::config::paths::global_agents_dir()` themselves — see #1385.
pub async fn wire_project_config_with_agents_dir(
    project_dir: &Path,
    agents_dir: &Path,
    native_store: NativeConfigStore,
    cli: CliConfigInput,
) -> Result<ConfigWiring, ConfigError> {
    log::debug!(
        target: crate::LOG_TARGET,
        "wire_project_config_with_agents_dir: enter (agents_dir={})",
        agents_dir.display()
    );
    let result = async {
        let canonical = project_dir
            .canonicalize()
            .map_err(|_| ConfigError::InvalidLocation(ProjectConfigLocationError::NotCanonical))?;
        let location = ProjectConfigLocation::try_from_project_identity(
            canonical.clone(),
            canonical.to_string_lossy().as_bytes(),
        )
        .map_err(ConfigError::InvalidLocation)?;
        let global_path = agents_dir.join(share::config::paths::NEW_CONFIG_FILE);
        let service = std::sync::Arc::new(
            ConfigAppService::with_global_path(Some(project_dir), global_path)
                .with_native_store(native_store),
        );
        service.set_project_location(location);
        service.set_cli_patch(CliArgsAdapter::read(&cli)).await;
        service.load().await.map_err(ConfigError::Load)?;
        Ok(ConfigWiring { service })
    }
    .await;
    match &result {
        Ok(_) => log::info!(
            target: crate::LOG_TARGET,
            "wire_project_config_with_agents_dir: success"
        ),
        Err(_) => log::warn!(
            target: crate::LOG_TARGET,
            "wire_project_config_with_agents_dir: failure"
        ),
    }
    result
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

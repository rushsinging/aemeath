//! Config：分层配置的读取、合并与提交（含 wire 工厂）。
//!
//! # Published Language（DDD 五类判据）
//!
//! | 类别 | 实体 | 消费者 |
//! |---|---|---|
//! | 命令 | `ConfigUpdateData`（SetModel/SetPermissionMode/SetMemoryConfig）、`CliConfigInputData` | composition、context、runtime、sdk |
//! | 事件 | `ConfigChangeData`、`ConfigChangeCauseData`、`ConfigSubscriptionData`、`ConfigRefreshOutcomeData` | cli、context、runtime、sdk |
//! | 值对象 | `ConfigFieldData`、`ProjectConfigLocationData`、`PreparedConfigUpdateData`、`PreparedProjectConfigData`、`GlobalConfigDocument`、`GlobalConfigRevision`、`BootstrapConfigReceipt` | cli、composition、context、runtime、sdk |
//! | 端口 | `ConfigReader`、`ConfigWriter`、`ProjectConfigParticipant`、`GlobalConfigConnectStore`、`NativeConfigStore`（注入式存储句柄，wire 签名载荷） | cli、composition、context、runtime、share |
//! | 工厂 | `wire_project_config`、`wire_project_config_with_cli`、`wire_project_config_with_agents_dir`、`wire_config_override_store`、`wire_global_connect_store`、`ConfigWiring` | composition、context、runtime |
//!
//! 子发布语言模块（模块即边界，跨界按模块=1 计数，不逐符号上根）：`catalog`、`connect`、`form`、
//! `ports`。`runtime_resolution` 不整体发布，仅两个 free fn（`resolve_provider_runtime` /
//! `resolve_provider_runtime_for_selection`）跨界；`user_agent`、`adapters`、`domain`、
//! `global_store`（`#[path]` 挂载）为 crate 内实现，根导出仅保留其真实跨界面。
//!
//! 判定记录：
//! - 死面已清：facade 分析 死 0 / crate 内根折返 0——`ResolvedProviderRuntimeConfig` 下架
//!   （类型保留在 `runtime_resolution` 模块内，是两个 free fn 的返回类型）；折返消费
//!   （`crate::GlobalConfigRevision`/`crate::CliConfigInputData`）全部改写为真实模块路径；
//! - 错误统一 `share::error::DomainError`：`ConfigRefreshError`/`ConfigPersistError`/
//!   `ConfigUpdateError`/`ConfigError`/`ProjectConfigLocationError` 全部 crate 内经 `From`
//!   折叠越界；错误家族仍有活跃 match 消费，统一折叠为单一错误属后续设计决策（本批记录
//!   判定不硬做）；
//! - Outcome Result 化：`ConfigRefreshOutcomeData` 的 Unchanged/Reloaded 为真领域状态，
//!   不再以 Err 伪装枚举；`ConfigPersistOutcomeData` 已随 persist 轨 Result 化删除；
//! - 实现体经 wire 工厂封装：`ConfigAppService`（生产零消费，跨 crate 仅测试直连，收窄随
//!   测试迁移批执行）、`NativeConfigStore::new`、`FilesystemGlobalConfigConnectStore::new`
//!   均 `pub(crate)`，crate 外只能经 `wire_*` 工厂取得；
//! - `is_persist_conflict`：CAS 冲突语义由 config 独占解释，供 composition commit 适配器
//!   经 `DomainError` 分流（消费方零内部变体 match）；
//! - `ConfigRefreshOutcomeData`（真领域状态）与 `ConfigChangeCauseData`（`ConfigChangeData`
//!   构造所需协议字段，runtime mapping 测试 crate 外构造）保留根导出；
//! - `CliArgsAdapter`/`EnvAdapter`/`FileAdapter` 为 wire 内部实现收窄 crate 内（此前消费
//!   分析误报：share/config 存在另一套同名独立实现，词命中假阳性）。

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:config";
mod adapters;
pub mod catalog;
pub mod connect;
mod domain;
#[path = "core/form.rs"]
pub mod form;
#[path = "gateway/global_store.rs"]
mod global_store;
pub mod ports;
pub mod runtime_resolution;
mod user_agent;

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod catalog_tests;

#[cfg(test)]
#[path = "user_agent_tests.rs"]
mod user_agent_tests;

#[cfg(test)]
#[path = "ports_tests.rs"]
mod ports_tests;

pub use adapters::{CliConfigInputData, ConfigAppService, NativeConfigStore};
// NativeConfigStore 归类角色（AuditStore 同判：注入式存储句柄，wire 签名载荷）。
// ConfigAppService 生产零消费（wire_project_config 内部构造）；跨 crate 引用均为
// 测试直连（owner-test-consumed 模式），收窄随测试迁移批（#1696）执行。
pub use domain::{
    ConfigChangeCauseData, ConfigChangeData, ConfigFieldData, ConfigRefreshOutcomeData,
    ConfigSubscriptionData, ConfigUpdateData, PreparedConfigUpdateData, PreparedProjectConfigData,
    ProjectConfigLocationData,
};
pub use global_store::{
    is_persist_conflict, BootstrapConfigReceipt, GlobalConfigConnectStore, GlobalConfigDocument,
    GlobalConfigRevision,
};
pub use ports::{ConfigReader, ConfigWriter, ProjectConfigParticipant};
pub use runtime_resolution::{resolve_provider_runtime, resolve_provider_runtime_for_selection};

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
pub fn wire_config_override_store(
    blob: std::sync::Arc<dyn storage::AtomicBlobPort>,
) -> NativeConfigStore {
    NativeConfigStore::new(blob)
}

/// Composition 的唯一 `GlobalConfigConnectStore` 构造入口：文件系统实现
/// （`FilesystemGlobalConfigConnectStore`）为 crate 内细节，其 `new` 与全部
/// 固有方法已收窄 `pub(crate)`，crate 外只能经此工厂拿到 trait 对象，
/// 实现细节在编译期不可达。
pub fn wire_global_connect_store(
    agents_dir: &Path,
) -> std::sync::Arc<dyn GlobalConfigConnectStore> {
    std::sync::Arc::new(global_store::FilesystemGlobalConfigConnectStore::new(
        agents_dir.to_path_buf(),
    ))
}

pub async fn wire_project_config_with_cli(
    project_dir: &Path,
    native_store: NativeConfigStore,
    cli: adapters::CliConfigInputData,
) -> Result<ConfigWiring, share::error::DomainError> {
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
        service
            .load()
            .await
            .map_err(|message| share::error::DomainError::storage("config", message))?;
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
) -> Result<ConfigWiring, share::error::DomainError> {
    let service = std::sync::Arc::new(ConfigAppService::for_project(project_dir, native_store)?);
    service
        .load()
        .await
        .map_err(|message| share::error::DomainError::storage("config", message))?;
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
    cli: adapters::CliConfigInputData,
) -> Result<ConfigWiring, share::error::DomainError> {
    log::debug!(
        target: crate::LOG_TARGET,
        "wire_project_config_with_agents_dir: enter (agents_dir={})",
        agents_dir.display()
    );
    let result = async {
        let canonical = project_dir.canonicalize().map_err(|_| {
            share::error::DomainError::from(domain::ConfigError::InvalidLocation(
                domain::ProjectConfigLocationError::NotCanonical,
            ))
        })?;
        let location = ProjectConfigLocationData::try_from_project_identity(
            canonical.clone(),
            canonical.to_string_lossy().as_bytes(),
        )?;
        let global_path = agents_dir.join(share::config::paths::NEW_CONFIG_FILE);
        let service = std::sync::Arc::new(
            ConfigAppService::with_global_path(Some(project_dir), global_path)
                .with_native_store(native_store),
        );
        service.set_project_location(location);
        service
            .set_cli_patch(crate::adapters::CliArgsAdapter::read(&cli))
            .await;
        service
            .load()
            .await
            .map_err(|message| share::error::DomainError::storage("config", message))?;
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

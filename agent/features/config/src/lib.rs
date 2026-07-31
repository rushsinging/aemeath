/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:config";
mod adapters;
mod application;
pub mod catalog;
pub mod connect;
mod contract;
#[path = "gateway/global_store.rs"]
mod global_store;
pub mod ports;
pub mod runtime_resolution;
pub mod user_agent;

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod catalog_tests;

#[cfg(test)]
#[path = "user_agent_tests.rs"]
mod user_agent_tests;

#[cfg(test)]
#[path = "ports_tests.rs"]
mod ports_tests;

pub use adapters::{
    encode_native_patch, merge_native_patches, CliArgsAdapter, CliConfigInput,
    CompatibilityAdapter, ConfigAdapterError, ConfigFormat, ConfigValidator, EnvAdapter, EnvSource,
    FileAdapter, NativeConfigStore, ProcessEnv,
};
pub use application::{wire_project_config, ConfigAppService, ConfigWiring};
pub use global_store::{
    BootstrapConfigReceipt, FilesystemGlobalConfigConnectStore, GlobalConfigCommitReceipt,
    GlobalConfigConnectStore, GlobalConfigDocument, GlobalConfigRevision, GlobalConfigStoreError,
};
pub use runtime_resolution::{
    resolve_provider_runtime, resolve_provider_runtime_for_selection, ProviderRuntimeResolver,
    ResolvedProviderRuntimeConfig,
};
pub use user_agent::{
    build_global_default_user_agent, resolve_provider_user_agent, ProviderUserAgentInputs,
};
pub async fn wire_project_config_with_cli(
    project_dir: &std::path::Path,
    native_store: NativeConfigStore,
    cli: CliConfigInput,
) -> Result<ConfigWiring, ConfigError> {
    application::wire_project_config_with_cli(project_dir, native_store, cli).await
}
pub async fn wire_project_config_with_agents_dir(
    project_dir: &std::path::Path,
    agents_dir: &std::path::Path,
    native_store: NativeConfigStore,
    cli: CliConfigInput,
) -> Result<ConfigWiring, ConfigError> {
    application::wire_project_config_with_agents_dir(project_dir, agents_dir, native_store, cli)
        .await
}
pub use contract::{
    ConfigChangeCause, ConfigChangeSet, ConfigCommitWarning, ConfigError, ConfigField,
    ConfigPersistError, ConfigPersistOutcome, ConfigQuery, ConfigQueryError, ConfigReader,
    ConfigRefreshError, ConfigRefreshOutcome, ConfigSubscription, ConfigUpdate, ConfigUpdateError,
    ConfigWriter, PreparedConfigUpdate, PreparedProjectConfig, ProjectConfigLocation,
    ProjectConfigLocationError, ProjectConfigParticipant, ReadyConfigCommit,
};

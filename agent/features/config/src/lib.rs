/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:config";
mod adapters;
mod application;
mod domain;
mod ports;

pub use adapters::{
    encode_native_patch, merge_native_patches, CliArgsAdapter, CliConfigInput,
    CompatibilityAdapter, ConfigAdapterError, ConfigFormat, ConfigValidator, EnvAdapter, EnvSource,
    FileAdapter, NativeConfigStore, ProcessEnv,
};
pub use application::{wire_project_config, ConfigAppService, ConfigWiring};
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
pub use domain::{
    ConfigChangeCause, ConfigChangeSet, ConfigCommitWarning, ConfigError, ConfigField,
    ConfigPersistError, ConfigPersistOutcome, ConfigQueryError, ConfigRefreshError,
    ConfigRefreshOutcome, ConfigSubscription, ConfigUpdate, ConfigUpdateError,
    PreparedConfigUpdate, PreparedProjectConfig, ProjectConfigLocation, ProjectConfigLocationError,
    ReadyConfigCommit,
};
pub use ports::{ConfigQuery, ConfigReader, ConfigWriter, ProjectConfigParticipant};

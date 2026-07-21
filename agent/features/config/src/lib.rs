/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:config";
mod adapters;
mod application;
mod contract;

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
pub use contract::{
    ConfigChangeCause, ConfigChangeSet, ConfigCommitWarning, ConfigError, ConfigField,
    ConfigPersistError, ConfigPersistOutcome, ConfigQuery, ConfigQueryError, ConfigReader,
    ConfigSubscription, ConfigUpdate, ConfigUpdateError, ConfigWriter, PreparedConfigUpdate,
    PreparedProjectConfig, ProjectConfigLocation, ProjectConfigLocationError,
    ProjectConfigParticipant, ReadyConfigCommit,
};

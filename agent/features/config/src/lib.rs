mod adapters;
mod application;
mod contract;

pub use adapters::{
    encode_native_config, CliArgsAdapter, CliConfigInput, CompatibilityAdapter, ConfigAdapterError,
    ConfigFormat, ConfigValidator, FileAdapter, NativeConfigStore,
};
pub use application::{wire_project_config, ConfigAppService, ConfigWiring};
pub async fn wire_project_config_with_cli(
    project_dir: &std::path::Path,
    cli: CliConfigInput,
) -> Result<ConfigWiring, ConfigError> {
    application::wire_project_config_with_cli(project_dir, cli).await
}
pub use contract::{
    ConfigChangeCause, ConfigChangeSet, ConfigCommitWarning, ConfigError, ConfigField,
    ConfigPersistError, ConfigPersistOutcome, ConfigQuery, ConfigQueryError, ConfigReader,
    ConfigSubscription, ConfigUpdate, ConfigUpdateError, ConfigWriter, PreparedConfigUpdate,
    PreparedProjectConfig, ProjectConfigLocation, ProjectConfigLocationError,
    ProjectConfigParticipant, ReadyConfigCommit,
};

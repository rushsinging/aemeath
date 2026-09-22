//! 出站 / 入站 port：config 对外发布的稳定接口契约。
//!
//! R8 方向：ports 只依赖 domain（`crate::domain`）；trait 签名引用的
//! 领域类型均定义在 domain.rs，NEVER 在本文件引入技术实现细节。
use crate::domain::{
    ConfigChangeSet, ConfigError, ConfigPersistOutcome, ConfigQueryError, ConfigRefreshOutcome,
    ConfigSubscription, ConfigUpdate, ConfigUpdateError, PreparedConfigUpdate,
    PreparedProjectConfig, ProjectConfigLocation, ReadyConfigCommit,
};
use async_trait::async_trait;
use share::config::domain::snapshot::ConfigSnapshot;
use tokio::sync::watch;

#[async_trait]
pub trait ConfigReader: Send + Sync {
    fn committed_snapshot(&self) -> ConfigSnapshot;
    fn subscribe_committed(&self) -> watch::Receiver<ConfigSnapshot>;
    async fn refresh_if_sources_changed(&self) -> ConfigRefreshOutcome;
}

#[async_trait]
pub trait ConfigQuery: Send + Sync {
    async fn snapshot(&self) -> Result<ConfigSnapshot, ConfigQueryError>;
    async fn subscribe(&self) -> Result<ConfigSubscription, ConfigQueryError>;
}

#[async_trait]
pub trait ConfigWriter: Send + Sync {
    async fn update(&self, command: ConfigUpdate) -> Result<ConfigChangeSet, ConfigUpdateError>;
}

#[async_trait]
pub trait ProjectConfigParticipant: Send + Sync {
    async fn prepare_for_project(
        &self,
        location: &ProjectConfigLocation,
    ) -> Result<PreparedProjectConfig, ConfigError>;
    fn snapshot(&self) -> ConfigSnapshot;
    async fn commit_project(&self, prepared: PreparedProjectConfig);
    async fn prepare_update(
        &self,
        command: ConfigUpdate,
    ) -> Result<PreparedConfigUpdate, ConfigUpdateError>;
    async fn persist_update(&self, prepared: PreparedConfigUpdate) -> ConfigPersistOutcome;
    fn commit_update(&self, ready: ReadyConfigCommit) -> ConfigChangeSet;
}

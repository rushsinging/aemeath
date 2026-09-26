//! 出站 / 入站 port：config 对外发布的稳定接口契约。
//!
//! R8 方向：ports 只依赖 domain（`crate::domain`）；trait 签名引用的
//! 领域类型均定义在 domain.rs，NEVER 在本文件引入技术实现细节。
use crate::domain::{
    ConfigChangeData, ConfigPersistOutcomeData, ConfigRefreshOutcomeData, ConfigSubscriptionData,
    ConfigUpdateData, PreparedConfigUpdateData, PreparedProjectConfigData,
    ProjectConfigLocationData, ReadyConfigCommitData,
};
use async_trait::async_trait;
use share::config::domain::snapshot::ConfigSnapshot;
use tokio::sync::watch;

#[async_trait]
pub trait ConfigReader: Send + Sync {
    fn committed_snapshot(&self) -> ConfigSnapshot;
    fn subscribe_committed(&self) -> watch::Receiver<ConfigSnapshot>;
    async fn refresh_if_sources_changed(&self) -> ConfigRefreshOutcomeData;
    /// 异步快照（gate-aware 视图实现经内部 gate 校验后委托）。
    async fn snapshot(&self) -> Result<ConfigSnapshot, share::error::DomainError>;
    /// 订阅（gate-aware 同上）。
    async fn subscribe(&self) -> Result<ConfigSubscriptionData, share::error::DomainError>;
}

#[async_trait]
pub trait ConfigWriter: Send + Sync {
    async fn update(
        &self,
        command: ConfigUpdateData,
    ) -> Result<ConfigChangeData, share::error::DomainError>;
}

#[async_trait]
pub trait ProjectConfigParticipant: Send + Sync {
    async fn prepare_for_project(
        &self,
        location: &ProjectConfigLocationData,
    ) -> Result<PreparedProjectConfigData, share::error::DomainError>;
    fn snapshot(&self) -> ConfigSnapshot;
    async fn commit_project(&self, prepared: PreparedProjectConfigData);
    async fn prepare_update(
        &self,
        command: ConfigUpdateData,
    ) -> Result<PreparedConfigUpdateData, share::error::DomainError>;
    async fn persist_update(&self, prepared: PreparedConfigUpdateData) -> ConfigPersistOutcomeData;
    fn commit_update(&self, ready: ReadyConfigCommitData) -> ConfigChangeData;
}

//! Connect 域装配入口（#1866 A1）：把原先散在 composition 的手拼收拢为单点。
//!
//! - `wire_connect`：catalog / 系统信息 / 全局 UA / 版本默认值全部由 crate 内
//!   提供，调用方只需给出 store 与 probe 实现；
//! - `GlobalConnectCommitAdapter`：store → `ConnectCommitPort`，revision 契约
//!   直接透传 `GlobalConfigRevision`（#1866 A3，NEVER 再手工哈希 u64）；
//! - `GlobalConnectDirectoryAdapter`：store → `ConnectProviderDirectory`，
//!   经 `connect::provider_snapshots_from_document` 类型化读取（#1866 A2）。

use std::sync::Arc;

use async_trait::async_trait;

use crate::connect::{ConnectAppService, ConnectAppServiceBuilder};
use crate::connect::{
    ConnectCommitError, ConnectCommitPort, ConnectCommitReceipt, ConnectCommitRequest,
    ConnectProviderDirectory, ExistingProviderSnapshot, PersistErrorKind,
};
use crate::ports::{ProviderProbePort, SystemInformation};
use crate::{GlobalConfigConnectStore, GlobalConfigDocument};

/// store → ConnectCommitPort：CAS 写入 + 语义化错误映射（唯一实现点）。
pub struct GlobalConnectCommitAdapter {
    store: Arc<dyn GlobalConfigConnectStore>,
}

impl GlobalConnectCommitAdapter {
    pub fn new(store: Arc<dyn GlobalConfigConnectStore>) -> Arc<Self> {
        Arc::new(Self { store })
    }
}

#[async_trait]
impl ConnectCommitPort for GlobalConnectCommitAdapter {
    async fn commit(
        &self,
        request: ConnectCommitRequest,
    ) -> Result<ConnectCommitReceipt, ConnectCommitError> {
        let revision = self
            .store
            .compare_and_swap(request.expected_global_revision, request.draft)
            .await
            .map_err(map_store_error)?;
        Ok(ConnectCommitReceipt {
            applied_revision: revision,
        })
    }
}

/// store → ConnectProviderDirectory：start_connect 单点加载已有 Provider 快照。
pub struct GlobalConnectDirectoryAdapter {
    store: Arc<dyn GlobalConfigConnectStore>,
}

impl GlobalConnectDirectoryAdapter {
    pub fn new(store: Arc<dyn GlobalConfigConnectStore>) -> Arc<Self> {
        Arc::new(Self { store })
    }
}

#[async_trait]
impl ConnectProviderDirectory for GlobalConnectDirectoryAdapter {
    async fn provider_snapshots(
        &self,
    ) -> Result<Vec<ExistingProviderSnapshot>, share::error::DomainError> {
        let document: Option<GlobalConfigDocument> = self.store.load_global_document().await?;
        match document {
            Some(document) => crate::connect::provider_snapshots_from_document(&document),
            None => Ok(Vec::new()),
        }
    }
}

/// 全局配置 `api.user_agent` 读取（#1866 A5 下沉）：文档结构异常降级为默认
/// 配置语义（不制造非法 UA）；空白视为未配置。
pub fn global_user_agent_from_document(document: &GlobalConfigDocument) -> Option<String> {
    let config = serde_json::from_value::<share::config::Config>(document.value.clone())
        .unwrap_or_else(|_| share::config::Config::default());
    let value = config.api.user_agent.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// 读取当前全局配置的全局 UA（Connect probe 与正式请求的共同回退来源）。
pub async fn load_global_user_agent(
    store: &Arc<dyn GlobalConfigConnectStore>,
) -> Result<Option<String>, share::error::DomainError> {
    let document = store.load_global_document().await?;
    Ok(document.as_ref().and_then(global_user_agent_from_document))
}

/// Connect 域装配入口（#1866 A1）：catalog / 系统信息 / 全局 UA / 版本默认值
/// 全部由 crate 内提供；调用方只注入 store 与 probe 实现。
pub async fn wire_connect(
    store: Arc<dyn GlobalConfigConnectStore>,
    probe: Arc<dyn ProviderProbePort>,
) -> Arc<ConnectAppService> {
    let commit = GlobalConnectCommitAdapter::new(Arc::clone(&store));
    let directory = GlobalConnectDirectoryAdapter::new(Arc::clone(&store));
    let global_user_agent = load_global_user_agent(&store).await.unwrap_or_default();
    let builder: ConnectAppServiceBuilder = ConnectAppService::builder()
        .with_catalog(crate::catalog::PROVIDER_CATALOG)
        .with_probe(probe)
        .with_commit(commit)
        .with_provider_directory(directory)
        .with_global_user_agent(global_user_agent)
        .with_system(SystemInformation {
            os_name: std::env::consts::OS.to_string(),
            os_version: None,
            arch: std::env::consts::ARCH.to_string(),
        })
        .with_version(share::version());
    Arc::new(builder.build())
}

/// DomainError → ConnectCommitError 语义映射（#1866 A3 下沉单点）。
fn map_store_error(error: share::error::DomainError) -> ConnectCommitError {
    if crate::is_persist_conflict(&error) {
        return ConnectCommitError::PersistConflict { expected: None };
    }
    match error.category() {
        share::error::ErrorCategory::Storage => ConnectCommitError::PersistFailed {
            kind: PersistErrorKind::Io,
            message: error.message().to_string(),
        },
        share::error::ErrorCategory::Invalid | share::error::ErrorCategory::Unavailable => {
            ConnectCommitError::PersistFailed {
                kind: PersistErrorKind::Serialization,
                message: error.message().to_string(),
            }
        }
    }
}

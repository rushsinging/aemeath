//! Context 测试支撑（cfg test / dev）：跨 crate 测试消费的 port mock。
//!
//! #2025 C3：原 `application/main_session.rs` 的 test_support 抽出；
//! `MainSessionWiring` 协调器已迁 runtime（`SessionSwitchCoordinator`），
//! 本模块只保留与 wiring 无关的 port mock。

use std::sync::Arc;

pub use crate::adapters::AtomicBlobSessionManagement;

use crate::domain::session::CanonicalSession;
use crate::ports::SessionManagementPort;
use async_trait::async_trait;
use memory::api::{MemoryOpener, MemoryOpenerError, MemoryPort, ProjectMemoryKey};

#[derive(Clone, Default)]
pub struct InMemoryTestOpener;

#[async_trait]
impl MemoryOpener for InMemoryTestOpener {
    async fn open_memory(
        &self,
        _key: &ProjectMemoryKey,
        _config: &share::config::MemoryConfig,
    ) -> Result<Arc<dyn MemoryPort>, MemoryOpenerError> {
        Ok(Arc::new(
            memory::api::InMemoryMemory::new(memory::api::MemoryPolicy::default())
                .expect("default MemoryPolicy is always valid"),
        ))
    }

    fn boxed_clone(&self) -> Box<dyn MemoryOpener> {
        Box::new(self.clone())
    }
}

#[derive(Default)]
pub struct UnavailableSessionManagement;

#[async_trait]
impl SessionManagementPort for UnavailableSessionManagement {
    async fn load_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<CanonicalSession, crate::domain::session::SessionManagementError> {
        Err(crate::domain::session::SessionManagementError::NotFound(
            id.to_string(),
        ))
    }

    async fn list_for_project(
        &self,
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<
        Vec<crate::domain::session::SessionListEntryData>,
        crate::domain::session::SessionManagementError,
    > {
        Ok(Vec::new())
    }

    async fn export_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<Vec<u8>, crate::domain::session::SessionManagementError> {
        Err(crate::domain::session::SessionManagementError::NotFound(
            id.to_string(),
        ))
    }

    async fn import_for_project(
        &self,
        _bytes: &[u8],
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<
        crate::domain::session::SessionListEntryData,
        crate::domain::session::SessionManagementError,
    > {
        Err(crate::domain::session::SessionManagementError::Storage(
            "test session port unavailable".to_string(),
        ))
    }

    async fn update_metadata_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentityData,
        _update: crate::domain::session::SessionMetadataUpdateData,
    ) -> Result<
        crate::domain::session::SessionListEntryData,
        crate::domain::session::SessionManagementError,
    > {
        Err(crate::domain::session::SessionManagementError::NotFound(
            id.to_string(),
        ))
    }

    async fn delete_for_project(
        &self,
        id: &str,
        _project: &share::session_types::ProjectIdentityData,
    ) -> Result<(), crate::domain::session::SessionManagementError> {
        Err(crate::domain::session::SessionManagementError::NotFound(
            id.to_string(),
        ))
    }
}

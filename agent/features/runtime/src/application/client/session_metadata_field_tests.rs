use super::*;
use context::session::{CanonicalSession, SessionMetadata};
use context::SessionManagementError;
use share::session_types::ProjectIdentityData;
use std::sync::Mutex;

#[derive(Default)]
struct RecordingSessionManagement {
    updates: Mutex<Vec<SessionMetadata>>,
    stored_reasoning_effort: Mutex<Option<String>>,
}

impl RecordingSessionManagement {
    fn session_with_reasoning_effort(id: &str, value: Option<String>) -> CanonicalSession {
        let mut session = CanonicalSession::fixture(id);
        session.metadata.reasoning_effort = value;
        session
    }
}

#[async_trait::async_trait]
impl context::SessionManagementPort for RecordingSessionManagement {
    async fn load_for_project(
        &self,
        id: &str,
        _project: &ProjectIdentityData,
    ) -> Result<CanonicalSession, SessionManagementError> {
        Ok(Self::session_with_reasoning_effort(
            id,
            self.stored_reasoning_effort.lock().unwrap().clone(),
        ))
    }

    async fn list_for_project(
        &self,
        _project: &ProjectIdentityData,
    ) -> Result<Vec<sdk::SessionItemList>, SessionManagementError> {
        Ok(Vec::new())
    }

    async fn export_for_project(
        &self,
        _id: &str,
        _project: &ProjectIdentityData,
    ) -> Result<Vec<u8>, SessionManagementError> {
        Err(SessionManagementError::Storage("unused".into()))
    }

    async fn import_for_project(
        &self,
        _bytes: &[u8],
        _project: &ProjectIdentityData,
    ) -> Result<sdk::SessionItemList, SessionManagementError> {
        Err(SessionManagementError::Storage("unused".into()))
    }

    async fn update_metadata_for_project(
        &self,
        _id: &str,
        _project: &ProjectIdentityData,
        update: SessionMetadata,
    ) -> Result<sdk::SessionItemList, SessionManagementError> {
        self.updates.lock().unwrap().push(update.clone());
        Ok(sdk::SessionItemList {
            id: "session-1".to_string(),
            title: None,
            project: None,
            model: update.model,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            message_count: 0,
            preview: None,
            summary: String::new(),
        })
    }

    async fn delete_for_project(
        &self,
        _id: &str,
        _project: &ProjectIdentityData,
    ) -> Result<(), SessionManagementError> {
        Err(SessionManagementError::Storage("unused".into()))
    }
}

#[tokio::test]
async fn persist_metadata_field_writes_reasoning_effort() {
    let port = RecordingSessionManagement::default();
    persist_metadata_field(
        &port,
        "session-1",
        &ProjectIdentityData::default(),
        SessionMetadataField::ReasoningEffort,
        "high",
    )
    .await;

    let updates = port.updates.lock().unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].reasoning_effort.as_deref(), Some("high"));
}

#[tokio::test]
async fn persist_metadata_field_skips_when_value_unchanged() {
    let port = RecordingSessionManagement {
        stored_reasoning_effort: Mutex::new(Some("high".to_string())),
        ..Default::default()
    };
    persist_metadata_field(
        &port,
        "session-1",
        &ProjectIdentityData::default(),
        SessionMetadataField::ReasoningEffort,
        "high",
    )
    .await;

    assert!(port.updates.lock().unwrap().is_empty());
}

#[tokio::test]
async fn persist_metadata_field_skips_blank_value() {
    let port = RecordingSessionManagement::default();
    persist_metadata_field(
        &port,
        "session-1",
        &ProjectIdentityData::default(),
        SessionMetadataField::ReasoningEffort,
        "   ",
    )
    .await;

    assert!(port.updates.lock().unwrap().is_empty());
}

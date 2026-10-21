use super::*;
use context::session::{CanonicalSession, SessionMetadata, SnapshotState};
use context::SessionManagementError;
use share::session_types::ProjectIdentityData;
use std::sync::Mutex;

#[derive(Default)]
struct RecordingSessionManagement {
    updates: Mutex<Vec<context::SessionMetadataUpdateData>>,
    stored_reasoning_effort: Mutex<Option<String>>,
}

impl RecordingSessionManagement {
    fn session_with_reasoning_effort(id: &str, value: Option<String>) -> CanonicalSession {
        CanonicalSession {
            id: id.to_string(),
            chats: vec![],
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            metadata: SessionMetadata {
                reasoning_effort: value,
                ..SessionMetadata::default()
            },
            tasks: SnapshotState::Missing,
            workspace: SnapshotState::Missing,
            revision: 1,
            compact: None,
            cleared_after: None,
            run_slices: Default::default(),
            committed_steps: Default::default(),
            skill_load_records: Vec::new(),
        }
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
    ) -> Result<Vec<context::SessionListEntryData>, SessionManagementError> {
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
    ) -> Result<context::SessionListEntryData, SessionManagementError> {
        Err(SessionManagementError::Storage("unused".into()))
    }

    async fn update_metadata_for_project(
        &self,
        _id: &str,
        _project: &ProjectIdentityData,
        update: context::SessionMetadataUpdateData,
    ) -> Result<context::SessionListEntryData, SessionManagementError> {
        self.updates.lock().unwrap().push(update.clone());
        Ok(context::SessionListEntryData {
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

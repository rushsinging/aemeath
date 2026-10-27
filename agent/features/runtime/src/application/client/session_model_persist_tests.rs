use super::*;
use context::session::{CanonicalSession, SessionMetadata};
use context::SessionManagementError;
use share::session_types::ProjectIdentityData;
use std::sync::Mutex;

#[derive(Default)]
struct RecordingSessionManagement {
    updates: Mutex<Vec<(String, Option<String>)>>,
    stored_model: Mutex<Option<String>>,
}

impl RecordingSessionManagement {
    fn with_stored_model(model: Option<&str>) -> Self {
        Self {
            updates: Mutex::new(Vec::new()),
            stored_model: Mutex::new(model.map(str::to_string)),
        }
    }

    fn session_with_model(id: &str, model: Option<String>) -> CanonicalSession {
        let mut session = CanonicalSession::empty(id.to_string());
        session.metadata = SessionMetadata {
            model,
            ..SessionMetadata::default()
        };
        session.revision = 1;
        session
    }
}

#[async_trait::async_trait]
impl context::SessionManagementPort for RecordingSessionManagement {
    async fn load_for_project(
        &self,
        id: &str,
        _project: &ProjectIdentityData,
    ) -> Result<context::session::CanonicalSession, SessionManagementError> {
        Ok(Self::session_with_model(
            id,
            self.stored_model.lock().unwrap().clone(),
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
        id: &str,
        _project: &ProjectIdentityData,
        update: context::session::SessionMetadata,
    ) -> Result<sdk::SessionItemList, SessionManagementError> {
        self.updates
            .lock()
            .unwrap()
            .push((id.to_string(), update.model.clone()));
        Ok(sdk::SessionItemList {
            id: id.to_string(),
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

#[test]
fn session_model_key_joins_source_and_model_id() {
    assert_eq!(
        session_model_key("Zhipu Coding Plan", "glm-5.3"),
        "Zhipu Coding Plan/glm-5.3"
    );
}

#[tokio::test]
async fn persist_session_model_writes_metadata_model() {
    let port = RecordingSessionManagement::default();
    let project = ProjectIdentityData {
        initial_cwd: "/tmp/proj".into(),
        git_common_dir: None,
    };
    persist_session_model(&port, "session-1", &project, "Zhipu Coding Plan/glm-5.3").await;
    let updates = port.updates.lock().unwrap().clone();
    assert_eq!(
        updates,
        vec![(
            "session-1".to_string(),
            Some("Zhipu Coding Plan/glm-5.3".to_string())
        )]
    );
}

#[tokio::test]
async fn persist_session_model_skips_blank_model() {
    let port = RecordingSessionManagement::default();
    let project = ProjectIdentityData {
        initial_cwd: "/tmp/proj".into(),
        git_common_dir: None,
    };
    persist_session_model(&port, "session-1", &project, "  ").await;
    assert!(port.updates.lock().unwrap().is_empty());
}

#[tokio::test]
async fn persist_session_model_skips_when_metadata_model_unchanged() {
    let port = RecordingSessionManagement::with_stored_model(Some("OmniRoute/cursor-auto"));
    let project = ProjectIdentityData {
        initial_cwd: "/tmp/proj".into(),
        git_common_dir: None,
    };
    persist_session_model(&port, "session-1", &project, "OmniRoute/cursor-auto").await;
    assert!(
        port.updates.lock().unwrap().is_empty(),
        "same model must not bump session generation"
    );
}

#[tokio::test]
async fn persist_session_model_writes_when_metadata_model_differs() {
    let port = RecordingSessionManagement::with_stored_model(Some("OmniRoute/old"));
    let project = ProjectIdentityData {
        initial_cwd: "/tmp/proj".into(),
        git_common_dir: None,
    };
    persist_session_model(&port, "session-1", &project, "OmniRoute/cursor-auto").await;
    let updates = port.updates.lock().unwrap().clone();
    assert_eq!(
        updates,
        vec![(
            "session-1".to_string(),
            Some("OmniRoute/cursor-auto".to_string())
        )]
    );
}

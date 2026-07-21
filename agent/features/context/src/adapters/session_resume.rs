use crate::domain::session::{SessionManagementError, SessionRestore, SessionResumeProjection};

impl crate::application::MainSessionWiring {
    pub async fn resume_session(
        &self,
        session_id: &str,
    ) -> Result<SessionResumeProjection, SessionManagementError> {
        let session = self.session_management().load_canonical(session_id).await?;
        self.resume_prepared(session)
            .await
            .map_err(|error| SessionManagementError::Resume(error.to_string()))?;
        let committed = self.committed_session();
        let restore = SessionRestore::from_canonical(&committed);
        Ok(SessionResumeProjection {
            session_id: committed.id.clone(),
            messages: restore.active_messages,
            created_at: restore.created_at,
            trimmed: restore.trimmed,
            repaired: restore.repaired,
        })
    }
}

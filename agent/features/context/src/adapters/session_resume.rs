use crate::domain::session::{SessionManagementError, SessionRestore, SessionResumeView};

impl crate::application::MainSessionWiring {
    pub async fn resume_session(
        &self,
        session_id: &str,
    ) -> Result<SessionResumeView, SessionManagementError> {
        let project = self.project_identity();
        log::debug!(
            target: crate::LOG_TARGET,
            "resume_lifecycle boundary=context_session_resume stage=session_load_started session_id={}",
            session_id
        );
        let session = self
            .session_management()
            .load_for_project(session_id, &project)
            .await?;
        log::debug!(
            target: crate::LOG_TARGET,
            "resume_lifecycle boundary=context_session_resume stage=session_load_completed session_id={} committed_steps={}",
            session_id,
            session.committed_steps.len()
        );
        log::debug!(
            target: crate::LOG_TARGET,
            "resume_lifecycle boundary=context_session_resume stage=resume_prepare_started session_id={}",
            session_id
        );
        self.resume_prepared(session)
            .await
            .map_err(|error| SessionManagementError::Resume(error.to_string()))?;
        log::debug!(
            target: crate::LOG_TARGET,
            "resume_lifecycle boundary=context_session_resume stage=resume_prepare_completed session_id={}",
            session_id
        );
        let committed = self.committed_session();
        let restore = SessionRestore::from_canonical(&committed);
        Ok(SessionResumeView {
            session_id: committed.id.clone(),
            active_messages: restore.active_messages,
            display_steps: restore.display_steps,
            compacted: restore.compacted,
            created_at: restore.created_at,
            trimmed: restore.trimmed,
            repaired: restore.repaired,
        })
    }
}

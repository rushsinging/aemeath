use std::time::Instant;

use crate::domain::session::{SessionManagementError, SessionRestore, SessionResumeView};

impl crate::application::MainSessionWiring {
    pub async fn resume_session(
        &self,
        session_id: &str,
    ) -> Result<SessionResumeView, SessionManagementError> {
        let started = Instant::now();
        let project = self.project_identity();
        log::debug!(
            target: crate::LOG_TARGET,
            "resume_lifecycle boundary=context_session_resume stage=session_load_started session_id={}",
            session_id
        );
        // Project scope remains mandatory; `load_for_resume` extends the same
        // `load_for_project` contract with a body-free display-history index.
        let loaded = self
            .session_management()
            .load_for_resume(session_id, &project)
            .await?;
        let display_history = loaded.display_history;
        let session = loaded.active_session;
        log::debug!(
            target: crate::LOG_TARGET,
            "resume_lifecycle boundary=context_session_resume stage=session_load_completed session_id={} committed_steps={} elapsed_ms={}",
            session_id,
            session.committed_steps.len(),
            started.elapsed().as_secs_f64() * 1000.0
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
        let restore_started = Instant::now();
        let committed = self.committed_session();
        let restore = if display_history.is_some() {
            SessionRestore::active_from_canonical(&committed)
        } else {
            SessionRestore::from_canonical(&committed)
        };
        log::debug!(
            target: crate::LOG_TARGET,
            "resume_lifecycle boundary=context_session_resume stage=restore_completed session_id={} display_index_steps={} legacy_steps={} active_messages={} elapsed_ms={} restore_ms={}",
            committed.id,
            display_history.as_ref().map_or(0, |index| index.steps().len()),
            restore.display_steps.len(),
            restore.active_messages.len(),
            started.elapsed().as_secs_f64() * 1000.0,
            restore_started.elapsed().as_secs_f64() * 1000.0
        );
        Ok(SessionResumeView {
            session_id: committed.id.clone(),
            active_messages: restore.active_messages,
            display_steps: restore.display_steps,
            display_history,
            created_at: restore.created_at,
            trimmed: restore.trimmed,
            repaired: restore.repaired,
        })
    }
}

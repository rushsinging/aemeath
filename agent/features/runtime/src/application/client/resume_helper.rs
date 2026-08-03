//! Shared resume helper for startup `args.resume` and runtime
//! `PendingCommand::ResumeSession`.

use crate::LOG_TARGET;

pub type ResumeError = context::SessionManagementError;

pub async fn resume_session_to_backing(
    session_id: &str,
    wiring: &context::MainSessionWiring,
) -> Result<context::SessionResumeView, ResumeError> {
    log::debug!(
        target: LOG_TARGET,
        "resume_lifecycle boundary=runtime_resume_helper stage=backing_load_started session_id={}",
        session_id
    );
    let resume_view = match wiring.resume_session(session_id).await {
        Ok(resume_view) => resume_view,
        Err(error) => {
            log::warn!(
                target: LOG_TARGET,
                "resume_lifecycle boundary=runtime_resume_helper stage=backing_load_failed session_id={} error={}",
                session_id,
                error
            );
            return Err(error);
        }
    };
    log::debug!(
        target: LOG_TARGET,
        "resume_lifecycle boundary=runtime_resume_helper stage=backing_load_completed requested_session_id={} loaded_session_id={} active_messages={} display_index_steps={} legacy_steps={} trimmed={} repaired={}",
        session_id,
        resume_view.session_id,
        resume_view.active_messages.len(),
        resume_view
            .display_history
            .as_ref()
            .map_or(0, |index| index.steps().len()),
        resume_view.display_steps.len(),
        resume_view.trimmed,
        resume_view.repaired
    );
    if resume_view.trimmed > 0 || resume_view.repaired > 0 {
        log::info!(
            target: LOG_TARGET,
            "resume {}: trimmed={} repaired={}",
            resume_view.session_id,
            resume_view.trimmed,
            resume_view.repaired
        );
    }
    Ok(resume_view)
}

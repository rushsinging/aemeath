//! Shared resume helper for startup `args.resume` and runtime
//! `PendingCommand::ResumeSession`.

use crate::LOG_TARGET;

pub type ResumeError = context::SessionManagementError;

pub async fn resume_session_to_backing(
    session_id: &str,
    wiring: &context::MainSessionWiring,
) -> Result<context::SessionResumeProjection, ResumeError> {
    log::debug!(
        target: LOG_TARGET,
        "resume_lifecycle boundary=runtime_resume_helper stage=backing_load_started session_id={}",
        session_id
    );
    let projection = match wiring.resume_session(session_id).await {
        Ok(projection) => projection,
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
        "resume_lifecycle boundary=runtime_resume_helper stage=backing_load_completed requested_session_id={} loaded_session_id={} active_messages={} display_steps={} trimmed={} repaired={}",
        session_id,
        projection.session_id,
        projection.active_messages.len(),
        projection.display_steps.len(),
        projection.trimmed,
        projection.repaired
    );
    if projection.trimmed > 0 || projection.repaired > 0 {
        log::info!(
            target: LOG_TARGET,
            "resume {}: trimmed={} repaired={}",
            projection.session_id,
            projection.trimmed,
            projection.repaired
        );
    }
    Ok(projection)
}

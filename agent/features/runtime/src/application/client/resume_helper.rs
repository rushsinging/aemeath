//! Shared resume helper for startup `args.resume` and runtime
//! `PendingCommand::ResumeSession`.

use crate::LOG_TARGET;

pub type ResumeError = context::SessionManagementError;

pub async fn resume_session_to_backing(
    session_id: &str,
    coordinator: &crate::SessionSwitchCoordinator,
) -> Result<context::session::SessionRestore, ResumeError> {
    log::debug!(
        target: LOG_TARGET,
        "resume_lifecycle boundary=runtime_resume_helper stage=backing_load_started session_id={}",
        session_id
    );
    let restore = match coordinator.resume_session(session_id).await {
        Ok(restore) => restore,
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
        restore.session_id,
        restore.active_messages.len(),
        restore
            .display_history
            .as_ref()
            .map_or(0, |index| index.steps().len()),
        restore.display_steps.len(),
        restore.trimmed,
        restore.repaired
    );
    if restore.trimmed > 0 || restore.repaired > 0 {
        log::info!(
            target: LOG_TARGET,
            "resume {}: trimmed={} repaired={}",
            restore.session_id,
            restore.trimmed,
            restore.repaired
        );
    }
    Ok(restore)
}

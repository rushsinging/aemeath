//! Shared resume helper for startup `args.resume` and runtime
//! `PendingCommand::ResumeSession`.

use crate::LOG_TARGET;

pub type ResumeError = context::SessionManagementError;

pub async fn resume_session_to_backing(
    session_id: &str,
    wiring: &context::MainSessionWiring,
) -> Result<context::SessionResumeProjection, ResumeError> {
    let projection = wiring.resume_session(session_id).await?;
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

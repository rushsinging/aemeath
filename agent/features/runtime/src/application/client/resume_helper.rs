//! Shared resume helper for startup `args.resume` and runtime
//! `PendingCommand::ResumeSession`.
//!
//! Both call sites go through [`resume_session_to_backing`]: load/upgrade the
//! canonical session and call `wiring.resume_prepared`.
//!
//! **CM5 fix**: if a [`SessionProjectionParticipant`] is registered with the
//! wiring (as it is after `from_args_with_workspace`), the leased projection
//! backing is updated **inside** the exclusive gate by the participant —
//! this helper does **not** write to the backing outside the gate. It only
//! returns a read-only [`SessionRestore`] derived from the committed session
//! so callers can emit the `SessionResumed` event.

use context::session::SessionRestore;

use crate::LOG_TARGET;

/// Structured error from the resume pipeline.
///
/// The `Load` variant preserves the original [`SessionLoadError`] so the
/// loop runner can map it to SDK `SessionResumeFailureKind`. The
/// `Coordinator` variant wraps [`MainSessionError`] from
/// `wiring.resume_prepared`.
#[derive(Debug)]
pub enum ResumeError {
    Load(context::session::SessionLoadError),
    Coordinator(context::MainSessionError),
}

impl std::fmt::Display for ResumeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(e) => write!(f, "{e:?}"),
            Self::Coordinator(e) => write!(f, "{e}"),
        }
    }
}

/// Resumes a session by ID through the MainSessionWiring coordinator.
///
/// Pipeline:
/// 1. Load + upgrade the canonical session from disk (`load_canonical_session`).
/// 2. Call `wiring.resume_prepared` under the exclusive admission permit.
///    If a [`SessionProjectionParticipant`] is registered, the leased
///    projection backing is updated **inside** the gate by the participant.
/// 3. Derive a read-only [`SessionRestore`] from the committed session for
///    event emission. This does **not** write to any backing.
///
/// Returns `(restore, session_id)`. The caller uses `restore` for the
/// `SessionResumed` event and updates its local chain variable; it does
/// **not** write to the shared `current_chain` / `frozen_chats` /
/// `active_summary` backing — that is already done by the participant.
pub async fn resume_session_to_backing(
    session_id: &str,
    wiring: &context::MainSessionWiring,
) -> Result<(SessionRestore, String), ResumeError> {
    // 1. Load and upgrade the canonical session from disk.
    let canonical = context::session::load_canonical_session(session_id)
        .await
        .map_err(ResumeError::Load)?;
    let resumed_id = canonical.id.clone();

    // 2. Atomically resume through the wiring coordinator. This acquires the
    //    exclusive admission permit, restores workspace/config/memory/tasks,
    //    publishes the new committed session, and — if a participant is
    //    registered — synchronously updates the leased projection backing,
    //    all before releasing the gate.
    wiring
        .resume_prepared(canonical)
        .await
        .map_err(ResumeError::Coordinator)?;

    // 3. Derive a read-only restore from the committed session for event
    //    emission. The projection backing is already up to date (updated by
    //    the participant inside the gate); we do NOT write to it here.
    let committed = wiring.committed_session();
    let restore = SessionRestore::from_canonical(&committed);

    if restore.trimmed > 0 || restore.repaired > 0 {
        log::info!(
            target: LOG_TARGET,
            "resume {}: trimmed={} repaired={}",
            resumed_id,
            restore.trimmed,
            restore.repaired
        );
    }

    Ok((restore, resumed_id))
}

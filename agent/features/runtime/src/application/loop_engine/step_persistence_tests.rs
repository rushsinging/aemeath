use super::resolve_commit_revision;
use crate::ports::{ContextPortError, SessionId, SessionRevision};
use sdk::RunStepId;

fn session_id() -> SessionId {
    SessionId::new("session-1")
}

fn step_id() -> RunStepId {
    RunStepId::new("step-1")
}

#[test]
fn current_revision_wins_over_stale_window_snapshot() {
    // 窗口快照 revision 早于本 step 自身的 accepted-input / 工具回执
    // 推进，收口必须用会话当前修订号，否则 CAS 误判为并发冲突。
    let resolved = resolve_commit_revision(
        SessionRevision::new(231),
        Ok(Some(SessionRevision::new(232))),
        &session_id(),
        &step_id(),
    );

    assert_eq!(resolved, SessionRevision::new(232));
}

#[test]
fn missing_current_revision_falls_back_to_snapshot() {
    let resolved =
        resolve_commit_revision(SessionRevision::new(7), Ok(None), &session_id(), &step_id());

    assert_eq!(resolved, SessionRevision::new(7));
}

#[test]
fn refresh_error_falls_back_to_snapshot() {
    let resolved = resolve_commit_revision(
        SessionRevision::new(9),
        Err(ContextPortError::SessionRepository(
            "backend down".to_string(),
        )),
        &session_id(),
        &step_id(),
    );

    assert_eq!(resolved, SessionRevision::new(9));
}

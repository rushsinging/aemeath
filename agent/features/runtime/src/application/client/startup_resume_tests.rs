//! Startup resume → SDK backing 映射的 L1 字段完整性测试（from_args 职责 1）。
//!
//! 锁定 `map_resume_view_to_sdk_backing` 的逐字段传递：
//! steps、display history index、created_at 毫秒解析、compacted 与 session id。
//! `FinalizeCause → SDK` 枚举映射的完备性由 `sdk_event_mapper_tests` 承担。

use std::sync::Arc;

use context::domain::FinalizeCause;
use context::{DisplayHistoryStepIndex, SessionRestoreStep, SessionResumeView};
use share::message::{Message, Role};

use super::map_resume_view_to_sdk_backing;

fn resume_step(
    run_id: &str,
    step_id: &str,
    finalize: Option<FinalizeCause>,
    duration_ms: Option<u64>,
) -> SessionRestoreStep {
    SessionRestoreStep {
        run_id: run_id.to_string(),
        step_id: step_id.to_string(),
        message_segments: vec![Arc::new([Message::placeholder(Role::User)])],
        finalize_cause: finalize,
        duration_ms,
    }
}

fn sample_resume_view() -> SessionResumeView {
    SessionResumeView {
        session_id: "session-42".to_string(),
        active_messages: vec![Message::placeholder(Role::Assistant)],
        display_steps: vec![
            resume_step(
                "run-1",
                "step-1",
                Some(FinalizeCause::Completed),
                Some(1200),
            ),
            resume_step(
                "run-1",
                "step-2",
                Some(FinalizeCause::UserCancelledStep),
                None,
            ),
        ],
        display_history: Some(DisplayHistoryStepIndex::fixture(
            "session-42",
            7,
            vec![("run-1", "step-1", "codex", 12)],
        )),
        compacted: true,
        created_at: "2026-07-21T10:30:00+08:00".to_string(),
        trimmed: 1,
        repaired: 0,
    }
}

#[test]
fn map_resume_view_preserves_step_fields_in_order() {
    let backing = map_resume_view_to_sdk_backing(sample_resume_view());

    assert_eq!(backing.session_id, "session-42");
    assert_eq!(backing.steps.len(), 2);
    assert_eq!(backing.steps[0].run_id, "run-1");
    assert_eq!(backing.steps[0].step_id, "step-1");
    assert_eq!(
        backing.steps[0].message_segments.len(),
        1,
        "message segments count must be preserved per step"
    );
    assert_eq!(
        backing.steps[0].finalize_cause,
        Some(sdk::ResumedStepFinalizeCause::Completed)
    );
    assert_eq!(backing.steps[0].duration_ms, Some(1200));
    assert_eq!(
        backing.steps[1].finalize_cause,
        Some(sdk::ResumedStepFinalizeCause::UserCancelledStep)
    );
    assert_eq!(backing.steps[1].duration_ms, None);
}

#[test]
fn map_resume_view_preserves_display_history_index_fields() {
    let backing = map_resume_view_to_sdk_backing(sample_resume_view());

    let index = backing
        .display_history
        .as_ref()
        .expect("display history index must be preserved");
    assert_eq!(index.session_id, "session-42");
    assert_eq!(index.generation_revision, 7);
    assert_eq!(index.steps.len(), 1);
    assert_eq!(index.steps[0].run_id, "run-1");
    assert_eq!(index.steps[0].step_id, "step-1");
    assert_eq!(index.steps[0].member_name, "codex");
    assert_eq!(index.steps[0].estimated_lines, 12);
}

#[test]
fn map_resume_view_parses_created_at_to_epoch_millis() {
    let backing = map_resume_view_to_sdk_backing(sample_resume_view());
    // 2026-07-21T10:30:00+08:00 == 2026-07-21T02:30:00Z
    assert!(
        backing.created_at > 0,
        "valid RFC3339 created_at must map to epoch millis"
    );
}

#[test]
fn map_resume_view_invalid_created_at_falls_back_to_zero() {
    let mut view = sample_resume_view();
    view.created_at = "not-a-timestamp".to_string();
    let backing = map_resume_view_to_sdk_backing(view);
    assert_eq!(
        backing.created_at, 0,
        "invalid created_at must degrade to 0 instead of failing bootstrap"
    );
}

#[test]
fn map_resume_view_without_display_history_keeps_none() {
    let mut view = sample_resume_view();
    view.display_history = None;
    let backing = map_resume_view_to_sdk_backing(view);
    assert!(backing.display_history.is_none());
    assert_eq!(backing.compacted, true);
}

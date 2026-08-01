use super::*;
use crate::domain::session::{
    AcceptedInputRecord, ActiveCompactMarker, CanonicalSession, CommittedRunSlice,
    CommittedRunStep, RunStepCursor, SnapshotState,
};
use share::message::{ContentBlock, Message, Role};
use std::path::Path;

fn unfinished_tool_session(state: crate::domain::ToolCallState) -> CanonicalSession {
    unfinished_tool_session_with_outcome(state, true, "sleep 180")
}

fn unfinished_tool_session_with_outcome(
    state: crate::domain::ToolCallState,
    has_outcome: bool,
    input_preview: &str,
) -> CanonicalSession {
    let identity = crate::domain::ToolCallIdentity {
        session_id: crate::domain::SessionId::new("session"),
        run_id: sdk::RunId::new("run-1"),
        step_id: sdk::RunStepId::new("step-1"),
        runtime_call_id: "runtime-call-1".to_string(),
        provider_call_id: Some("provider-call-1".to_string()),
        tool_name: "Bash".to_string(),
        call_index: 0,
        agent: false,
    };
    let mut receipt = crate::domain::ToolCallReceipt::pending(identity.clone(), input_preview);
    if state == crate::domain::ToolCallState::Running {
        receipt = receipt
            .advance(crate::domain::ToolReceiptMutation::running(identity))
            .unwrap()
            .receipt;
    }
    CanonicalSession {
        id: "session".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Missing,
        revision: 0,
        compact: None,
        run_slices: vec![CommittedRunSlice::new(
            "run-1",
            vec![CommittedRunStep {
                step_id: "step-1".to_string(),
                accepted_input: None,
                outcome: has_outcome.then(|| crate::domain::session::FinalizedOutcomeRecord {
                    finalize_cause: crate::domain::FinalizeCause::UserCancelledStep,
                    duration_ms: Some(7_325_000),
                    messages: vec![Message {
                        role: Role::Assistant,
                        content: vec![ContentBlock::ToolUse {
                            id: "provider-call-1".to_string(),
                            name: "Bash".to_string(),
                            input: serde_json::json!({"command": "sleep 180"}),
                        }],
                        metadata: None,
                    }]
                    .into(),
                    receipts: Vec::new(),
                    api_input_tokens: None,
                    fingerprint: "fp".to_string(),
                    committed_revision: 1,
                }),
                tool_receipts: vec![receipt],
            }],
        )],
        committed_steps: vec![],
        skill_load_records: Vec::new(),
    }
}

fn two_step_session() -> CanonicalSession {
    CanonicalSession {
        id: "session".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Missing,
        revision: 0,
        compact: None,
        run_slices: vec![CommittedRunSlice::new(
            "run-1",
            vec![
                CommittedRunStep::accepted_only(
                    "step-1",
                    AcceptedInputRecord::new(vec![Message::user("first")], "fp-1", 0),
                ),
                CommittedRunStep::accepted_only(
                    "step-2",
                    AcceptedInputRecord::new(vec![Message::user("second")], "fp-2", 0),
                ),
            ],
        )],
        committed_steps: vec![],
        skill_load_records: Vec::new(),
    }
}

#[test]
fn restore_preserves_run_step_boundaries_for_display_projection() {
    let restore = SessionRestore::from_canonical(&two_step_session());

    assert_eq!(restore.display_steps.len(), 2);
    assert_eq!(restore.display_steps[0].run_id, "run-1");
    assert_eq!(restore.display_steps[0].step_id, "step-1");
    assert_eq!(restore.display_steps[0].messages[0].text_content(), "first");
    assert_eq!(restore.display_steps[1].run_id, "run-1");
    assert_eq!(restore.display_steps[1].step_id, "step-2");
    assert_eq!(
        restore.display_steps[1].messages[0].text_content(),
        "second"
    );
}

#[test]
fn restore_projects_unfinished_tool_receipts_as_unconfirmed_results() {
    for state in [
        crate::domain::ToolCallState::Pending,
        crate::domain::ToolCallState::Running,
    ] {
        let restore = SessionRestore::from_canonical(&unfinished_tool_session(state));

        assert_eq!(restore.trimmed, 0);
        assert_eq!(restore.display_steps.len(), 1);
        assert_eq!(
            restore.display_steps[0].finalize_cause,
            Some(crate::domain::FinalizeCause::UserCancelledStep)
        );
        assert_eq!(restore.display_steps[0].duration_ms, Some(7_325_000));
        assert_eq!(restore.display_steps[0].messages.len(), 2);
        let result = &restore.display_steps[0].messages[1];
        assert_eq!(result.role, Role::User);
        let [ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        }] = result.content.as_slice()
        else {
            panic!("未完成 receipt 应恢复为 provider-safe tool_result");
        };
        assert_eq!(tool_use_id, "provider-call-1");
        assert!(*is_error);
        assert!(content.to_string().contains("CancellationUnconfirmed"));
    }
}

#[test]
fn restore_reconstructs_receipt_only_unfinished_tool_as_terminated_message_pair() {
    for state in [
        crate::domain::ToolCallState::Pending,
        crate::domain::ToolCallState::Running,
    ] {
        let session = unfinished_tool_session_with_outcome(
            state,
            false,
            r#"{"command":"sleep 180","timeout":120000}"#,
        );
        let restore = SessionRestore::from_canonical(&session);

        assert_eq!(restore.trimmed, 0);
        assert_eq!(restore.display_steps.len(), 1);
        let step = &restore.display_steps[0];
        assert_eq!(
            step.finalize_cause,
            Some(crate::domain::FinalizeCause::RunTerminated)
        );
        assert_eq!(step.messages.len(), 2);
        let [ContentBlock::ToolUse { id, name, input }] = step.messages[0].content.as_slice()
        else {
            panic!("receipt-only Step 应重建最小 ToolUse");
        };
        assert_eq!(step.messages[0].role, Role::Assistant);
        assert_eq!(id, "provider-call-1");
        assert_eq!(name, "Bash");
        assert_eq!(input["command"], "sleep 180");
        let [ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        }] = step.messages[1].content.as_slice()
        else {
            panic!("receipt-only Step 应补充 CancellationUnconfirmed ToolResult");
        };
        assert_eq!(step.messages[1].role, Role::User);
        assert_eq!(tool_use_id, "provider-call-1");
        assert!(*is_error);
        assert!(content.to_string().contains("CancellationUnconfirmed"));
    }
}

#[test]
fn restore_uses_safe_empty_input_when_receipt_preview_is_invalid() {
    let session = unfinished_tool_session_with_outcome(
        crate::domain::ToolCallState::Running,
        false,
        "{truncated",
    );

    let first = SessionRestore::from_canonical(&session);
    let second = SessionRestore::from_canonical(&session);

    assert_eq!(first.display_steps.len(), 1);
    assert_eq!(second.display_steps.len(), 1);
    let [ContentBlock::ToolUse { input, .. }] =
        first.display_steps[0].messages[0].content.as_slice()
    else {
        panic!("无效 input preview 仍应重建 ToolUse");
    };
    assert_eq!(input, &serde_json::json!({}));
    assert_eq!(
        serde_json::to_value(&first.display_steps[0].messages).unwrap(),
        serde_json::to_value(&second.display_steps[0].messages).unwrap()
    );
}

#[test]
fn real_terminated_session_restores_every_unfinished_bash_receipt() {
    let path = Path::new(env!("HOME")).join(".agents/session/019fa7d0-d769-7ad9-b7a2-fbec2113d147");
    if !path.exists() {
        return;
    }
    let bytes = std::fs::read(&path).expect("读取真实回归 Session");
    let decoded = crate::adapters::decode_session(&bytes).expect("解码真实回归 Session");
    let unfinished = decoded
        .session
        .run_slices
        .iter()
        .flat_map(|slice| slice.steps.iter())
        .flat_map(|step| step.tool_receipts.iter())
        .filter(|receipt| receipt.identity.tool_name == "Bash")
        .filter(|receipt| {
            matches!(
                receipt.state,
                crate::domain::ToolCallState::Pending | crate::domain::ToolCallState::Running
            )
        })
        .map(|receipt| provider_call_id(receipt).to_string())
        .collect::<Vec<_>>();
    assert!(
        !unfinished.is_empty(),
        "真实回归 Session 应包含未完成 Bash receipt"
    );

    let restore = SessionRestore::from_canonical(&decoded.session);

    for call_id in unfinished {
        let step = restore
            .display_steps
            .iter()
            .find(|step| {
                step.messages
                    .iter()
                    .any(|message| message.tool_use_ids().contains(&call_id.as_str()))
            })
            .expect("每个未完成 Bash receipt 都应出现在恢复投影");
        assert!(step.messages.iter().any(|message| {
            message
                .tool_result_ids()
                .into_iter()
                .any(|id| id == call_id)
        }));
        assert!(step.finalize_cause.is_some());
    }
}

#[test]
fn restore_reads_only_steps_from_active_marker() {
    let session = CanonicalSession {
        id: "session".to_string(),
        chats: vec![],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        metadata: Default::default(),
        tasks: SnapshotState::Missing,
        workspace: SnapshotState::Missing,
        revision: 0,
        compact: Some(ActiveCompactMarker {
            summary: "summary".to_string(),
            start_at: Some(RunStepCursor {
                run_id: "run-2".to_string(),
                step_id: "step-2".to_string(),
            }),
            source_revision: 0,
        }),
        run_slices: vec![
            CommittedRunSlice::new(
                "run-1",
                vec![CommittedRunStep::accepted_only(
                    "step-1",
                    AcceptedInputRecord::new(vec![Message::user("hidden")], "fp-1", 0),
                )],
            ),
            CommittedRunSlice::new(
                "run-2",
                vec![CommittedRunStep::accepted_only(
                    "step-2",
                    AcceptedInputRecord::new(vec![Message::user("visible")], "fp-2", 0),
                )],
            ),
        ],
        committed_steps: vec![],
        skill_load_records: Vec::new(),
    };

    let restore = SessionRestore::from_canonical(&session);

    assert_eq!(restore.active_messages.len(), 1);
    assert_eq!(restore.active_messages[0].text_content(), "visible");
    assert_eq!(restore.display_steps.len(), 2);
    assert_eq!(restore.display_steps[0].run_id, "run-1");
    assert_eq!(restore.display_steps[0].step_id, "step-1");
    assert_eq!(
        restore.display_steps[0].messages[0].text_content(),
        "hidden"
    );
    assert_eq!(restore.display_steps[1].run_id, "run-2");
    assert_eq!(restore.display_steps[1].step_id, "step-2");
    assert_eq!(
        restore.display_steps[1].messages[0].text_content(),
        "visible"
    );
}

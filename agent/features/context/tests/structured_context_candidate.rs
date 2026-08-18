use std::sync::Arc;

use share::message::{ContentBlock, Message};

use context::compact::{
    microcompact_exploration, snip_superseded_exploration, ContextReadCandidate, ContextReadRun,
    ContextReadStep, ProtectedRunPolicy,
};
use context::domain::session::{
    AcceptedInputRecord, CommittedRunSlice, CommittedRunStep, CommittedStepMessages,
    FinalizedOutcomeRecord, SessionHistory,
};
use context::domain::{
    CleanupConfirmation, FinalizeCause, SessionId, ToolCallIdentity, ToolCallReceipt,
    ToolOutcomeKind, ToolTerminalReceipt,
};

fn finalized(messages: Vec<Message>) -> FinalizedOutcomeRecord {
    FinalizedOutcomeRecord {
        finalize_cause: FinalizeCause::Completed,
        duration_ms: None,
        messages: messages.into(),
        receipts: Vec::new(),
        api_input_tokens: None,
        fingerprint: "fixture".into(),
        committed_revision: 1,
    }
}

fn accepted(text: &str) -> AcceptedInputRecord {
    AcceptedInputRecord::new(vec![Message::user(text)], format!("fp-{text}"), 1)
}

fn completed_run(run_id: &str, user_text: &str) -> CommittedRunSlice {
    let normalized_run_id = sdk::RunId::new(run_id);
    let normalized_step_id = sdk::RunStepId::new(format!("step-{run_id}"));
    CommittedRunSlice::new(
        normalized_run_id.as_ref(),
        vec![CommittedRunStep {
            step_id: normalized_step_id.as_str().to_string(),
            accepted_input: Some(accepted(user_text)),
            outcome: Some(finalized(vec![Message {
                role: share::message::Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: "done".into(),
                }],
                metadata: None,
            }])),
            tool_receipts: Vec::new(),
        }],
    )
}

#[test]
fn candidate_protects_three_recent_complete_runs_by_identity_not_user_role() {
    let history = SessionHistory::from_slices(vec![
        completed_run("run-1", "same role"),
        completed_run("run-2", "same role"),
        completed_run("run-3", "same role"),
        completed_run("run-4", "same role"),
    ]);

    let candidate = ContextReadCandidate::from_history(
        &history,
        sdk::RunId::new("run-5").as_ref(),
        ProtectedRunPolicy::latest_complete_runs(3),
    );

    assert!(!candidate_run(&candidate, "run-1").unwrap().is_protected());
    assert!(candidate_run(&candidate, "run-2").unwrap().is_protected());
    assert!(candidate_run(&candidate, "run-3").unwrap().is_protected());
    assert!(candidate_run(&candidate, "run-4").unwrap().is_protected());
}

#[test]
fn candidate_protects_active_run_and_unfinalized_step_without_position_inference() {
    let unfinished = CommittedRunSlice::new(
        sdk::RunId::new("run-unfinished").as_ref(),
        vec![CommittedRunStep::accepted_only(
            sdk::RunStepId::new("step-unfinished").as_str(),
            accepted("not necessarily last"),
        )],
    );
    let active = completed_run("run-active", "active");
    let history =
        SessionHistory::from_slices(vec![unfinished, completed_run("run-old", "old"), active]);

    let candidate = ContextReadCandidate::from_history(
        &history,
        sdk::RunId::new("run-active").as_ref(),
        ProtectedRunPolicy::latest_complete_runs(0),
    );

    assert!(candidate_run(&candidate, "run-unfinished")
        .unwrap()
        .is_protected());
    assert!(!candidate_run(&candidate, "run-old").unwrap().is_protected());
    assert!(candidate_run(&candidate, "run-active")
        .unwrap()
        .is_protected());
}

fn tool_message(
    tool_use_id: &str,
    tool_name: &str,
    input: serde_json::Value,
    text: &str,
) -> Message {
    Message {
        role: share::message::Role::Assistant,
        content: vec![
            ContentBlock::ToolUse {
                id: tool_use_id.into(),
                name: tool_name.into(),
                input,
            },
            ContentBlock::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: serde_json::json!({"typed": text}),
                is_error: false,
                text: Some(text.into()),
            },
        ],
        metadata: None,
    }
}

fn terminal_receipt(
    run_id: &str,
    step_id: &str,
    tool_use_id: &str,
    tool_name: &str,
    input: serde_json::Value,
    outcome: ToolOutcomeKind,
) -> ToolCallReceipt {
    ToolCallReceipt {
        identity: ToolCallIdentity {
            session_id: SessionId::new("session"),
            run_id: sdk::RunId::new(run_id),
            step_id: sdk::RunStepId::new(step_id),
            runtime_call_id: tool_use_id.into(),
            provider_call_id: Some(tool_use_id.into()),
            tool_name: tool_name.into(),
            call_index: 0,
            agent: false,
        },
        input_preview: input.to_string(),
        state: context::domain::ToolCallState::Terminal(ToolTerminalReceipt::new(
            outcome,
            "terminal",
            CleanupConfirmation::NotApplicable,
        )),
    }
}

fn tool_step_with_receipt_preview(
    run_id: &str,
    step_id: &str,
    tool_use_id: &str,
    tool_name: &str,
    input: serde_json::Value,
    receipt_input_preview: &str,
    text: &str,
    outcome: ToolOutcomeKind,
) -> ContextReadStep {
    let normalized_run_id = sdk::RunId::new(run_id);
    let normalized_step_id = sdk::RunStepId::new(step_id);
    let mut receipt = terminal_receipt(
        normalized_run_id.as_ref(),
        normalized_step_id.as_str(),
        tool_use_id,
        tool_name,
        input.clone(),
        outcome,
    );
    receipt.input_preview = receipt_input_preview.into();
    ContextReadStep::new(
        normalized_step_id.as_str(),
        None,
        Some(CommittedStepMessages::from(vec![tool_message(
            tool_use_id,
            tool_name,
            input,
            text,
        )])),
        vec![receipt],
        true,
    )
}

fn tool_step(
    run_id: &str,
    step_id: &str,
    tool_use_id: &str,
    tool_name: &str,
    input: serde_json::Value,
    text: &str,
    outcome: ToolOutcomeKind,
) -> ContextReadStep {
    let receipt_input_preview = input.to_string();
    tool_step_with_receipt_preview(
        run_id,
        step_id,
        tool_use_id,
        tool_name,
        input,
        &receipt_input_preview,
        text,
        outcome,
    )
}

fn context_read_run(run_id: &str, steps: Vec<ContextReadStep>) -> ContextReadRun {
    ContextReadRun::new(sdk::RunId::new(run_id).as_ref(), steps)
}

fn context_read_step(
    run_id: &str,
    step_id: &str,
    messages: Option<CommittedStepMessages>,
    receipts: Vec<ToolCallReceipt>,
    finalized: bool,
) -> ContextReadStep {
    let normalized_run_id = sdk::RunId::new(run_id);
    let normalized_step_id = sdk::RunStepId::new(step_id);
    let normalized_receipts = receipts
        .into_iter()
        .map(|mut receipt| {
            receipt.identity.run_id = normalized_run_id.clone();
            receipt.identity.step_id = normalized_step_id.clone();
            receipt
        })
        .collect();
    ContextReadStep::new(
        normalized_step_id.as_str(),
        None,
        messages,
        normalized_receipts,
        finalized,
    )
}

fn candidate_run<'candidate>(
    candidate: &'candidate ContextReadCandidate,
    run_id: &str,
) -> Option<&'candidate ContextReadRun> {
    let normalized_run_id = sdk::RunId::new(run_id);
    candidate
        .runs()
        .iter()
        .find(|run| run.run_id() == normalized_run_id.as_ref())
}

fn llm_text_for(candidate: &ContextReadCandidate, tool_use_id: &str) -> String {
    candidate
        .messages()
        .iter()
        .flat_map(|message| message.content.iter())
        .find_map(|block| match block {
            ContentBlock::ToolResult {
                tool_use_id: candidate_id,
                text,
                ..
            } if candidate_id == tool_use_id => text.clone(),
            _ => None,
        })
        .unwrap()
}

#[test]
fn snip_replaces_old_read_when_later_successful_edit_targets_same_canonical_path() {
    let old_read = tool_step(
        "run-read",
        "step-read",
        "read-call",
        "Read",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "obsolete source bytes",
        ToolOutcomeKind::Success,
    );
    let later_edit = tool_step(
        "run-edit",
        "step-edit",
        "edit-call",
        "Edit",
        serde_json::json!({"file_path": "/repo/src/../src/lib.rs"}),
        "edited",
        ToolOutcomeKind::Success,
    );
    let candidate = ContextReadCandidate::from_steps(
        vec![
            context_read_run("run-read", vec![old_read]),
            context_read_run("run-edit", vec![later_edit]),
        ],
        "run-active",
        ProtectedRunPolicy::latest_complete_runs(0),
    );

    let snipped = snip_superseded_exploration(&candidate);

    assert_eq!(
        llm_text_for(&snipped, "read-call"),
        "[Superseded tool result: Read /repo/src/lib.rs]"
    );
    let message = &candidate_run(&snipped, "run-read").unwrap().steps()[0].outcome_messages()[0];
    assert!(matches!(
        &message.content[0],
        ContentBlock::ToolUse { id, name, input }
            if id == "read-call"
                && name == "Read"
                && *input == serde_json::json!({"file_path": "/repo/src/lib.rs"})
    ));
    assert!(matches!(
        &message.content[1],
        ContentBlock::ToolResult { tool_use_id, content, is_error: false, text: Some(text) }
            if tool_use_id == "read-call"
                && *content == serde_json::json!({
                    "aemeath_context": {
                        "kind": "superseded_exploration",
                        "path": "/repo/src/lib.rs",
                        "tool": "Read"
                    }
                })
                && text == "[Superseded tool result: Read /repo/src/lib.rs]"
    ));
    assert!(!serde_json::to_string(message)
        .unwrap()
        .contains("obsolete source bytes"));
}

#[test]
fn snip_uses_typed_tool_input_when_receipt_preview_is_truncated() {
    let long_pattern = "x".repeat(600);
    let read_input = serde_json::json!({
        "file_path": "/repo/src/lib.rs",
        "pattern": long_pattern,
    });
    let old_read = tool_step_with_receipt_preview(
        "run-read",
        "step-read",
        "read-call",
        "Read",
        read_input,
        "{\"file_path\":\"/repo/src/lib.rs\",\"pattern\":\"truncated",
        "obsolete source bytes",
        ToolOutcomeKind::Success,
    );
    let later_write = tool_step(
        "run-write",
        "step-write",
        "write-call",
        "Write",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "written",
        ToolOutcomeKind::Success,
    );
    let candidate = ContextReadCandidate::from_steps(
        vec![
            context_read_run("run-read", vec![old_read]),
            context_read_run("run-write", vec![later_write]),
        ],
        "run-active",
        ProtectedRunPolicy::latest_complete_runs(0),
    );

    let snipped = snip_superseded_exploration(&candidate);

    assert_eq!(
        llm_text_for(&snipped, "read-call"),
        "[Superseded tool result: Read /repo/src/lib.rs]"
    );
}

#[test]
fn snip_keeps_different_path_failed_write_and_protected_run() {
    let old_read = tool_step(
        "run-read",
        "step-read",
        "read-call",
        "Read",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "keep me",
        ToolOutcomeKind::Success,
    );
    let protected_read = tool_step(
        "run-protected",
        "step-protected",
        "protected-read-call",
        "Read",
        serde_json::json!({"file_path": "/repo/src/protected.rs"}),
        "protected result",
        ToolOutcomeKind::Success,
    );
    let failed_write = tool_step(
        "run-write",
        "step-write",
        "write-call",
        "Write",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "failed",
        ToolOutcomeKind::Failure,
    );
    let other_edit = tool_step(
        "run-other",
        "step-other",
        "edit-call",
        "Edit",
        serde_json::json!({"file_path": "/repo/src/other.rs"}),
        "edited",
        ToolOutcomeKind::Success,
    );
    let protected_edit = tool_step(
        "run-edit-protected",
        "step-edit-protected",
        "protected-edit-call",
        "Edit",
        serde_json::json!({"file_path": "/repo/src/protected.rs"}),
        "edited protected file",
        ToolOutcomeKind::Success,
    );
    let candidate = ContextReadCandidate::from_steps(
        vec![
            context_read_run("run-read", vec![old_read]),
            context_read_run("run-write", vec![failed_write]),
            context_read_run("run-other", vec![other_edit]),
            context_read_run("run-protected", vec![protected_read]),
            context_read_run("run-edit-protected", vec![protected_edit]),
        ],
        "run-active",
        ProtectedRunPolicy::latest_complete_runs(2),
    );

    let snipped = snip_superseded_exploration(&candidate);

    assert_eq!(llm_text_for(&snipped, "read-call"), "keep me");
    assert_eq!(
        llm_text_for(&snipped, "protected-read-call"),
        "protected result"
    );
}

#[test]
fn snip_copies_only_the_matching_step_and_keeps_other_blocks_byte_stable() {
    let read_message = tool_message(
        "read-call",
        "Read",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "obsolete source bytes",
    );
    let untouched_block = ContentBlock::Text {
        text: "surrounding block".into(),
    };
    let mixed_message = Message {
        content: read_message
            .content
            .into_iter()
            .chain(std::iter::once(untouched_block.clone()))
            .collect(),
        ..read_message
    };
    let source_backing: Arc<[Message]> = vec![mixed_message].into();
    let old_read = ContextReadStep::new(
        "step-read",
        None,
        Some(CommittedStepMessages::from(
            source_backing.iter().cloned().collect::<Vec<_>>(),
        )),
        vec![terminal_receipt(
            "run-read",
            "step-read",
            "read-call",
            "Read",
            serde_json::json!({"file_path": "/repo/src/lib.rs"}),
            ToolOutcomeKind::Success,
        )],
        true,
    );
    let unchanged_read = tool_step(
        "run-unchanged",
        "step-unchanged",
        "other-read-call",
        "Read",
        serde_json::json!({"file_path": "/repo/src/other.rs"}),
        "still current",
        ToolOutcomeKind::Success,
    );
    let unchanged_backing = unchanged_read.outcome_messages();
    let later_write = tool_step(
        "run-write",
        "step-write",
        "write-call",
        "Write",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "written",
        ToolOutcomeKind::Success,
    );
    let candidate = ContextReadCandidate::from_steps(
        vec![
            context_read_run("run-read", vec![old_read]),
            context_read_run("run-unchanged", vec![unchanged_read]),
            context_read_run("run-write", vec![later_write]),
        ],
        "run-active",
        ProtectedRunPolicy::latest_complete_runs(0),
    );

    let snipped = snip_superseded_exploration(&candidate);

    let changed_backing =
        candidate_run(&snipped, "run-read").unwrap().steps()[0].outcome_messages();
    assert!(!Arc::ptr_eq(&source_backing, &changed_backing));
    assert!(matches!(
        &changed_backing[0].content[2],
        ContentBlock::Text { text } if text == "surrounding block"
    ));
    let retained_backing =
        candidate_run(&snipped, "run-unchanged").unwrap().steps()[0].outcome_messages();
    assert!(Arc::ptr_eq(&unchanged_backing, &retained_backing));
}

#[test]
fn snip_requires_tool_use_name_to_match_typed_receipt_identity() {
    let mut mismatched_read = tool_message(
        "read-call",
        "Edit",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "must stay",
    );
    mismatched_read.content.push(ContentBlock::Text {
        text: "surrounding".into(),
    });
    let read_step = ContextReadStep::new(
        "step-read",
        None,
        Some(CommittedStepMessages::from(vec![mismatched_read])),
        vec![terminal_receipt(
            "run-read",
            "step-read",
            "read-call",
            "Read",
            serde_json::json!({"file_path": "/repo/src/lib.rs"}),
            ToolOutcomeKind::Success,
        )],
        true,
    );
    let later_write = tool_step(
        "run-write",
        "step-write",
        "write-call",
        "Write",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "written",
        ToolOutcomeKind::Success,
    );
    let source_backing = read_step.outcome_messages();
    let candidate = ContextReadCandidate::from_steps(
        vec![
            context_read_run("run-read", vec![read_step]),
            context_read_run("run-write", vec![later_write]),
        ],
        "run-active",
        ProtectedRunPolicy::latest_complete_runs(0),
    );

    let snipped = snip_superseded_exploration(&candidate);

    assert_eq!(llm_text_for(&snipped, "read-call"), "must stay");
    assert!(Arc::ptr_eq(
        &source_backing,
        &candidate_run(&snipped, "run-read").unwrap().steps()[0].outcome_messages()
    ));
}

#[test]
fn snip_requires_later_write_receipt_to_belong_to_its_candidate_step() {
    let old_read = tool_step(
        "run-read",
        "step-read",
        "read-call",
        "Read",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "must stay",
        ToolOutcomeKind::Success,
    );
    let mut foreign_write = tool_step(
        "foreign-run",
        "foreign-step",
        "write-call",
        "Write",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "written",
        ToolOutcomeKind::Success,
    );
    foreign_write = ContextReadStep::new(
        "step-write",
        None,
        Some(CommittedStepMessages::from(
            foreign_write
                .outcome_messages()
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
        )),
        foreign_write.tool_receipts().to_vec(),
        true,
    );
    let candidate = ContextReadCandidate::from_steps(
        vec![
            context_read_run("run-read", vec![old_read]),
            context_read_run("run-write", vec![foreign_write]),
        ],
        "run-active",
        ProtectedRunPolicy::latest_complete_runs(0),
    );

    let snipped = snip_superseded_exploration(&candidate);

    assert_eq!(llm_text_for(&snipped, "read-call"), "must stay");
}

#[test]
fn microcompact_replaces_only_unprotected_exploration_whitelist_results() {
    let tool_names = [
        "Read",
        "Grep",
        "Glob",
        "WebFetch",
        "WebSearch",
        "LS",
        "ToolSearch",
    ];
    let old_steps = tool_names
        .iter()
        .enumerate()
        .map(|(tool_index, tool_name)| {
            tool_step(
                "run-old",
                &format!("step-{tool_index}"),
                &format!("call-{tool_index}"),
                tool_name,
                serde_json::json!({"path": format!("/repo/item-{tool_index}")}),
                &format!("old {tool_name} result"),
                ToolOutcomeKind::Success,
            )
        })
        .collect();
    let edit_step = tool_step(
        "run-edit",
        "step-edit",
        "edit-call",
        "Edit",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "edit result",
        ToolOutcomeKind::Success,
    );
    let bash_step = tool_step(
        "run-bash",
        "step-bash",
        "bash-call",
        "Bash",
        serde_json::json!({"command": "pwd"}),
        "bash result",
        ToolOutcomeKind::Success,
    );
    let candidate = ContextReadCandidate::from_steps(
        vec![
            context_read_run("run-old", old_steps),
            context_read_run("run-edit", vec![edit_step]),
            context_read_run("run-bash", vec![bash_step]),
        ],
        "run-active",
        ProtectedRunPolicy::latest_complete_runs(0),
    );

    let compacted = microcompact_exploration(&candidate);

    for (tool_index, tool_name) in tool_names.iter().enumerate() {
        assert_eq!(
            llm_text_for(&compacted, &format!("call-{tool_index}")),
            format!("[Microcompacted tool result: {tool_name}]")
        );
    }
    assert_eq!(llm_text_for(&compacted, "edit-call"), "edit result");
    assert_eq!(llm_text_for(&compacted, "bash-call"), "bash result");
}

#[test]
fn microcompact_protects_three_recent_complete_runs_and_unfinalized_run() {
    let run = |run_id: &str, finalized: bool| {
        let normalized_step_id = sdk::RunStepId::new(format!("step-{run_id}"));
        context_read_run(
            run_id,
            vec![context_read_step(
                run_id,
                normalized_step_id.as_str(),
                Some(CommittedStepMessages::from(vec![tool_message(
                    &format!("call-{run_id}"),
                    "Read",
                    serde_json::json!({"file_path": format!("/repo/{run_id}.rs")}),
                    &format!("result {run_id}"),
                )])),
                vec![terminal_receipt(
                    run_id,
                    &format!("step-{run_id}"),
                    &format!("call-{run_id}"),
                    "Read",
                    serde_json::json!({"file_path": format!("/repo/{run_id}.rs")}),
                    ToolOutcomeKind::Success,
                )],
                finalized,
            )],
        )
    };
    let candidate = ContextReadCandidate::from_steps(
        vec![
            run("old", true),
            run("recent-1", true),
            run("unfinished", false),
            run("recent-2", true),
            run("recent-3", true),
        ],
        "active-outside-history",
        ProtectedRunPolicy::latest_complete_runs(3),
    );

    let compacted = microcompact_exploration(&candidate);

    assert_eq!(
        llm_text_for(&compacted, "call-old"),
        "[Microcompacted tool result: Read]"
    );
    for run_id in ["recent-1", "unfinished", "recent-2", "recent-3"] {
        assert_eq!(
            llm_text_for(&compacted, &format!("call-{run_id}")),
            format!("result {run_id}")
        );
    }
}

#[test]
fn microcompact_uses_receipt_tool_identity_instead_of_untrusted_tool_use_name() {
    let mut mismatched_message = tool_message(
        "read-call",
        "Edit",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "must stay",
    );
    mismatched_message.content.push(ContentBlock::Text {
        text: "surrounding".into(),
    });
    let step = ContextReadStep::new(
        "step-read",
        None,
        Some(CommittedStepMessages::from(vec![mismatched_message])),
        vec![terminal_receipt(
            "run-read",
            "step-read",
            "read-call",
            "Read",
            serde_json::json!({"file_path": "/repo/src/lib.rs"}),
            ToolOutcomeKind::Success,
        )],
        true,
    );
    let source_backing = step.outcome_messages();
    let candidate = ContextReadCandidate::from_steps(
        vec![context_read_run("run-read", vec![step])],
        "run-active",
        ProtectedRunPolicy::latest_complete_runs(0),
    );

    let compacted = microcompact_exploration(&candidate);

    assert_eq!(llm_text_for(&compacted, "read-call"), "must stay");
    assert!(Arc::ptr_eq(
        &source_backing,
        &candidate_run(&compacted, "run-read").unwrap().steps()[0].outcome_messages()
    ));
}

#[test]
fn microcompact_requires_receipt_to_belong_to_its_candidate_run_and_step() {
    let message = tool_message(
        "read-call",
        "Read",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "must stay",
    );
    let step = ContextReadStep::new(
        "step-read",
        None,
        Some(CommittedStepMessages::from(vec![message])),
        vec![terminal_receipt(
            "foreign-run",
            "foreign-step",
            "read-call",
            "Read",
            serde_json::json!({"file_path": "/repo/src/lib.rs"}),
            ToolOutcomeKind::Success,
        )],
        true,
    );
    let source_backing = step.outcome_messages();
    let candidate = ContextReadCandidate::from_steps(
        vec![context_read_run("run-read", vec![step])],
        "run-active",
        ProtectedRunPolicy::latest_complete_runs(0),
    );

    let compacted = microcompact_exploration(&candidate);

    assert_eq!(llm_text_for(&compacted, "read-call"), "must stay");
    assert!(Arc::ptr_eq(
        &source_backing,
        &candidate_run(&compacted, "run-read").unwrap().steps()[0].outcome_messages()
    ));
}

#[test]
fn microcompact_preserves_protected_failed_and_unpaired_results_with_shared_backing() {
    let failed_read = tool_step(
        "run-failed",
        "step-failed",
        "failed-read-call",
        "Read",
        serde_json::json!({"file_path": "/repo/failed.rs"}),
        "failed read result",
        ToolOutcomeKind::Failure,
    );
    let unpaired_message = Message {
        role: share::message::Role::Assistant,
        content: vec![ContentBlock::ToolResult {
            tool_use_id: "unpaired-call".into(),
            content: serde_json::json!({"typed": "unpaired result"}),
            is_error: false,
            text: Some("unpaired result".into()),
        }],
        metadata: None,
    };
    let unpaired_step = ContextReadStep::new(
        "step-unpaired",
        None,
        Some(CommittedStepMessages::from(vec![unpaired_message])),
        vec![terminal_receipt(
            "run-unpaired",
            "step-unpaired",
            "unpaired-call",
            "Read",
            serde_json::json!({"file_path": "/repo/unpaired.rs"}),
            ToolOutcomeKind::Success,
        )],
        true,
    );
    let protected_read = tool_step(
        "run-protected",
        "step-protected",
        "protected-read-call",
        "Read",
        serde_json::json!({"file_path": "/repo/protected.rs"}),
        "protected read result",
        ToolOutcomeKind::Success,
    );
    let failed_backing = failed_read.outcome_messages();
    let unpaired_backing = unpaired_step.outcome_messages();
    let protected_backing = protected_read.outcome_messages();
    let candidate = ContextReadCandidate::from_steps(
        vec![
            context_read_run("run-failed", vec![failed_read]),
            context_read_run("run-unpaired", vec![unpaired_step]),
            context_read_run("run-protected", vec![protected_read]),
        ],
        sdk::RunId::new("run-protected").as_ref(),
        ProtectedRunPolicy::latest_complete_runs(0),
    );

    let compacted = microcompact_exploration(&candidate);

    assert_eq!(
        llm_text_for(&compacted, "failed-read-call"),
        "failed read result"
    );
    assert_eq!(llm_text_for(&compacted, "unpaired-call"), "unpaired result");
    assert_eq!(
        llm_text_for(&compacted, "protected-read-call"),
        "protected read result"
    );
    assert!(Arc::ptr_eq(
        &failed_backing,
        &candidate_run(&compacted, "run-failed").unwrap().steps()[0].outcome_messages()
    ));
    assert!(Arc::ptr_eq(
        &unpaired_backing,
        &candidate_run(&compacted, "run-unpaired").unwrap().steps()[0].outcome_messages()
    ));
    assert!(Arc::ptr_eq(
        &protected_backing,
        &candidate_run(&compacted, "run-protected").unwrap().steps()[0].outcome_messages()
    ));
}

#[test]
fn microcompact_is_idempotent_after_l2_snip_without_replacing_snip_metadata() {
    let old_read = tool_step(
        "run-read",
        "step-read",
        "read-call",
        "Read",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "obsolete source bytes",
        ToolOutcomeKind::Success,
    );
    let later_write = tool_step(
        "run-write",
        "step-write",
        "write-call",
        "Write",
        serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        "written",
        ToolOutcomeKind::Success,
    );
    let candidate = ContextReadCandidate::from_steps(
        vec![
            context_read_run("run-read", vec![old_read]),
            context_read_run("run-write", vec![later_write]),
        ],
        "run-active",
        ProtectedRunPolicy::latest_complete_runs(0),
    );
    let snipped = snip_superseded_exploration(&candidate);
    let snipped_backing =
        candidate_run(&snipped, "run-read").unwrap().steps()[0].outcome_messages();

    let compacted = microcompact_exploration(&snipped);

    assert_eq!(
        llm_text_for(&compacted, "read-call"),
        "[Superseded tool result: Read /repo/src/lib.rs]"
    );
    assert!(Arc::ptr_eq(
        &snipped_backing,
        &candidate_run(&compacted, "run-read").unwrap().steps()[0].outcome_messages()
    ));
}

#[test]
fn unchanged_steps_keep_shared_message_backing() {
    let messages: Arc<[Message]> = vec![Message {
        role: share::message::Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "shared".into(),
        }],
        metadata: Default::default(),
    }]
    .into();
    let history = SessionHistory::from_slices(vec![CommittedRunSlice::new(
        sdk::RunId::new("run-old").as_ref(),
        vec![CommittedRunStep::outcome_only(
            sdk::RunStepId::new("step-old").as_str(),
            finalized(messages.iter().cloned().collect()),
        )],
    )]);
    let source_backing = history.slices()[0].steps[0]
        .outcome
        .as_ref()
        .unwrap()
        .messages
        .as_arc();

    let candidate = ContextReadCandidate::from_history(
        &history,
        sdk::RunId::new("run-active").as_ref(),
        ProtectedRunPolicy::latest_complete_runs(0),
    );
    let candidate_backing =
        candidate_run(&candidate, "run-old").unwrap().steps()[0].outcome_messages();

    assert!(Arc::ptr_eq(&source_backing, &candidate_backing));
}

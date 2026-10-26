use crate::InMemorySessionRepository;
use crate::SessionRepository;
use crate::{
    AcceptedInputAppendData, AcceptedInputError, ContextAppendData, ContextAppendError,
    FinalizeCause, SessionRevision, ToolCallIdentityData, ToolOutcomeKindData,
    ToolReceiptMutationData, ToolTerminalReceiptData,
};
use sdk::RunId;
use share::ids::InteractionRequestId;
use share::ids::{RunStepId, SessionId};
use share::message::{ContentBlock, Message};
use tools::CleanupConfirmation;

fn append() -> ContextAppendData {
    ContextAppendData {
        session_id: SessionId::new("session"),
        expected_revision: SessionRevision::new(0),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        source_request_id: InteractionRequestId::new("request"),
        finalize_cause: FinalizeCause::Completed,
        duration_ms: None,
        messages: vec![Message::user("fact")],
        receipts: vec![],
        api_input_tokens: None,
    }
}

fn accepted_input() -> AcceptedInputAppendData {
    AcceptedInputAppendData {
        session_id: SessionId::new("session"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        source_request_id: InteractionRequestId::new("request"),
        messages: vec![Message::user("accepted")],
    }
}

fn tool_identity(
    run_id: &str,
    step_id: &str,
    call_id: &str,
    tool_name: &str,
) -> ToolCallIdentityData {
    ToolCallIdentityData {
        session_id: SessionId::new("session"),
        run_id: RunId::new(run_id),
        step_id: RunStepId::new(step_id),
        runtime_call_id: call_id.into(),
        provider_call_id: Some(call_id.into()),
        tool_name: tool_name.into(),
        call_index: 0,
        agent: false,
    }
}

fn tool_message(call_id: &str, tool_name: &str, path: &str, text: &str) -> Message {
    Message {
        role: share::message::Role::Assistant,
        content: vec![
            ContentBlock::ToolUse {
                id: call_id.into(),
                name: tool_name.into(),
                input: serde_json::json!({"file_path": path}),
            },
            ContentBlock::ToolResult {
                tool_use_id: call_id.into(),
                content: serde_json::json!({"typed": text}),
                is_error: false,
                text: Some(text.into()),
            },
        ],
        metadata: None,
    }
}

async fn commit_tool_step(
    backing: &InMemorySessionRepository,
    run_id: &str,
    step_id: &str,
    call_id: &str,
    tool_name: &str,
    path: &str,
    text: &str,
) {
    let session_id = SessionId::new("session");
    let current_revision = backing.snapshot(&session_id).await.unwrap().revision;
    let identity = tool_identity(run_id, step_id, call_id, tool_name);
    backing
        .advance_tool_receipt(ToolReceiptMutationData::pending(
            identity.clone(),
            serde_json::json!({"file_path": path}).to_string(),
        ))
        .await
        .unwrap();
    backing
        .advance_tool_receipt(ToolReceiptMutationData::terminal(
            identity,
            ToolTerminalReceiptData::new(
                ToolOutcomeKindData::Success,
                "terminal",
                CleanupConfirmation::NotApplicable,
            ),
        ))
        .await
        .unwrap();
    let receipt_revision = backing.snapshot(&session_id).await.unwrap().revision;
    backing
        .append_finalized(&ContextAppendData {
            session_id,
            expected_revision: current_revision.max(receipt_revision),
            run_id: RunId::new(run_id),
            step_id: RunStepId::new(step_id),
            source_request_id: InteractionRequestId::new(format!("request-{step_id}")),
            finalize_cause: FinalizeCause::Completed,
            duration_ms: None,
            messages: vec![tool_message(call_id, tool_name, path, text)],
            receipts: vec![],
            api_input_tokens: None,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn build_window_applies_l3_to_isolated_subagent_history() {
    use crate::ContextApplicationService;
    use crate::{ContextPromptSource, ContextWindowPort, PromptMaterialization};
    use crate::{ContextRequestData, Language};
    use share::config::domain::snapshot::ConfigSnapshot;
    use share::config::Config;
    use share::reasoning::ReasoningLevel;
    use std::sync::Arc;

    struct Prompt;
    #[async_trait::async_trait]
    impl ContextPromptSource for Prompt {
        async fn materialize(
            &self,
            _request: &ContextRequestData,
        ) -> Result<PromptMaterialization, crate::PromptMaterializationError> {
            Ok(PromptMaterialization {
                cacheable: vec![],
                uncached: vec![],
            })
        }
    }

    let backing = Arc::new(InMemorySessionRepository::new());
    let session_id = SessionId::new("session");
    backing.seed(&session_id, SessionRevision::new(0), vec![], None);
    for run_id in ["old", "recent-1", "recent-2", "recent-3"] {
        commit_tool_step(
            backing.as_ref(),
            run_id,
            &format!("step-{run_id}"),
            &format!("call-{run_id}"),
            "Read",
            &format!("/repo/{run_id}.rs"),
            &format!("result {run_id}"),
        )
        .await;
    }
    let service = ContextApplicationService::new(
        backing,
        Arc::new(Prompt),
        Arc::new(crate::NoOpContextMemorySource),
    );
    let request = ContextRequestData {
        session_id,
        request_id: InteractionRequestId::new("request-window"),
        run_id: RunId::new("active"),
        step_id: RunStepId::new("active-step"),
        pending_messages: vec![],
        workspace_root: std::path::PathBuf::from("/tmp/workspace"),
        workspace_is_git: false,
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::En,
        agent_roles: Default::default(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_total_tokens: None,
        heuristic_calibration: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    };

    let window = service.build_window(&request).await.unwrap();
    let result_text = |call_id: &str| {
        window
            .messages
            .iter()
            .flat_map(|message| message.content.iter())
            .find_map(|block| match block {
                ContentBlock::ToolResult {
                    tool_use_id, text, ..
                } if tool_use_id == call_id => text.clone(),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(
        result_text("call-old"),
        "[Microcompacted tool result: Read]"
    );
    for run_id in ["recent-1", "recent-2", "recent-3"] {
        assert_eq!(
            result_text(&format!("call-{run_id}")),
            format!("result {run_id}")
        );
    }
}

#[tokio::test]
async fn snapshot_preserves_structured_run_step_receipts_for_isolated_context() {
    let backing = InMemorySessionRepository::new();
    let session_id = SessionId::new("session");
    backing.seed(&session_id, SessionRevision::new(0), vec![], None);
    let identity = tool_identity("run", "step", "read-call", "Read");
    backing
        .append_accepted_input(&accepted_input())
        .await
        .unwrap();
    backing
        .advance_tool_receipt(ToolReceiptMutationData::pending(
            identity.clone(),
            r#"{"file_path":"/repo/src/lib.rs"}"#,
        ))
        .await
        .unwrap();
    backing
        .advance_tool_receipt(ToolReceiptMutationData::terminal(
            identity,
            ToolTerminalReceiptData::new(
                ToolOutcomeKindData::Success,
                "terminal",
                CleanupConfirmation::NotApplicable,
            ),
        ))
        .await
        .unwrap();
    let mut outcome = append();
    outcome.expected_revision = SessionRevision::new(3);
    backing.append_finalized(&outcome).await.unwrap();

    let snapshot = backing.snapshot(&session_id).await.unwrap();
    let history = snapshot
        .structured_history
        .expect("isolated context must retain structured history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].run_id, RunId::new("run").as_ref());
    assert_eq!(history[0].steps[0].step_id, RunStepId::new("step").as_str());
    assert!(history[0].steps[0].accepted_input.is_some());
    assert!(history[0].steps[0].outcome.is_some());
    assert_eq!(history[0].steps[0].tool_receipts.len(), 1);
}

#[tokio::test]
async fn accepted_input_has_independent_idempotency_from_finalized_outcome() {
    let backing = InMemorySessionRepository::new();
    let session_id = SessionId::new("session");
    backing.seed(&session_id, SessionRevision::new(0), vec![], None);

    let input = accepted_input();
    let receipt = backing.append_accepted_input(&input).await.unwrap();
    assert_eq!(receipt.committed_revision, SessionRevision::new(1));
    assert_eq!(
        backing.append_accepted_input(&input).await.unwrap(),
        receipt
    );

    let mut outcome = append();
    outcome.expected_revision = SessionRevision::new(1);
    backing.append_finalized(&outcome).await.unwrap();
    assert_eq!(
        backing.snapshot(&session_id).await.unwrap().messages.len(),
        2
    );

    let mut conflict = input;
    conflict.messages = vec![Message::user("conflicting input")];
    assert!(matches!(
        backing.append_accepted_input(&conflict).await,
        Err(AcceptedInputError::ContentConflict { .. })
    ));
}

#[tokio::test]
async fn finalized_outcome_keeps_receipt_metadata_for_idempotent_retry() {
    let backing = InMemorySessionRepository::new();
    let session_id = SessionId::new("session");
    backing.seed(&session_id, SessionRevision::new(0), vec![], None);
    let mut outcome = append();
    outcome.finalize_cause = FinalizeCause::RunTerminated;
    outcome.api_input_tokens = Some(21);
    outcome.receipts = vec![crate::RunStepReceiptData {
        call_id: "agent-call".to_string(),
        index: 0,
        outcome: crate::ToolOutcomeKindData::CancellationUnconfirmed,
        agent: true,
        summary: None,
        artifact_refs: Vec::new(),
        possible_side_effects: Vec::new(),
        unfinished_call_ids: Vec::new(),
    }];

    let first = backing.append_finalized(&outcome).await.unwrap();
    let second = backing.append_finalized(&outcome).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(first.committed_revision, SessionRevision::new(1));
    assert_eq!(
        backing.snapshot(&session_id).await.unwrap().messages[0].text_content(),
        "fact"
    );
}

#[tokio::test]
async fn same_step_and_fingerprint_is_idempotent() {
    let backing = InMemorySessionRepository::new();
    backing.seed(
        &SessionId::new("session"),
        SessionRevision::new(0),
        vec![],
        None,
    );
    let first = backing.append_finalized(&append()).await.unwrap();
    let second = backing.append_finalized(&append()).await.unwrap();
    assert_eq!(first, second);
    assert_eq!(
        backing
            .snapshot(&SessionId::new("session"))
            .await
            .unwrap()
            .messages
            .len(),
        1
    );
}

#[tokio::test]
async fn same_step_and_different_fingerprint_conflicts() {
    let backing = InMemorySessionRepository::new();
    backing.seed(
        &SessionId::new("session"),
        SessionRevision::new(0),
        vec![],
        None,
    );
    backing.append_finalized(&append()).await.unwrap();
    let mut conflicting = append();
    conflicting.messages = vec![Message::user("conflicting")];
    assert!(matches!(
        backing.append_finalized(&conflicting).await,
        Err(ContextAppendError::ContentConflict { .. })
    ));
}

use context::adapters::InMemorySessionRepository;
use context::domain::{
    AcceptedInputAppend, AcceptedInputError, CleanupConfirmation, ContentFingerprint,
    ContextAppend, ContextAppendError, ContextRequestId, FinalizeCause, RunStepId, SessionId,
    SessionRevision, ToolCallIdentity, ToolOutcomeKind, ToolReceiptMutation, ToolTerminalReceipt,
};
use context::ports::SessionRepository;
use sdk::RunId;
use share::message::{ContentBlock, Message};

fn append(fingerprint: &str) -> ContextAppend {
    ContextAppend {
        session_id: SessionId::new("session"),
        expected_revision: SessionRevision::new(0),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        source_request_id: ContextRequestId::new("request"),
        finalize_cause: FinalizeCause::Completed,
        duration_ms: None,
        messages: vec![Message::user("fact")],
        receipts: vec![],
        api_input_tokens: None,
        fingerprint: ContentFingerprint::new(fingerprint),
    }
}

fn accepted_input(fingerprint: &str) -> AcceptedInputAppend {
    AcceptedInputAppend {
        session_id: SessionId::new("session"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        source_request_id: ContextRequestId::new("request"),
        messages: vec![Message::user("accepted")],
        fingerprint: ContentFingerprint::new(fingerprint),
    }
}

fn tool_identity(run_id: &str, step_id: &str, call_id: &str, tool_name: &str) -> ToolCallIdentity {
    ToolCallIdentity {
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
        .advance_tool_receipt(ToolReceiptMutation::pending(
            identity.clone(),
            serde_json::json!({"file_path": path}).to_string(),
        ))
        .await
        .unwrap();
    backing
        .advance_tool_receipt(ToolReceiptMutation::terminal(
            identity,
            ToolTerminalReceipt::new(
                ToolOutcomeKind::Success,
                "terminal",
                CleanupConfirmation::NotApplicable,
            ),
        ))
        .await
        .unwrap();
    let receipt_revision = backing.snapshot(&session_id).await.unwrap().revision;
    backing
        .append_finalized(&ContextAppend {
            session_id,
            expected_revision: current_revision.max(receipt_revision),
            run_id: RunId::new(run_id),
            step_id: RunStepId::new(step_id),
            source_request_id: ContextRequestId::new(format!("request-{step_id}")),
            finalize_cause: FinalizeCause::Completed,
            duration_ms: None,
            messages: vec![tool_message(call_id, tool_name, path, text)],
            receipts: vec![],
            api_input_tokens: None,
            fingerprint: ContentFingerprint::new(format!("fp-{step_id}")),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn build_window_applies_l3_to_isolated_subagent_history() {
    use context::application::ContextApplicationService;
    use context::domain::{ContextRequest, Language, SystemPromptSpec};
    use context::ports::{ContextPort, ContextPromptSource, PromptMaterialization};
    use provider::ReasoningLevel;
    use share::config::domain::snapshot::ConfigSnapshot;
    use share::config::Config;
    use std::sync::Arc;

    struct Prompt;
    #[async_trait::async_trait]
    impl ContextPromptSource for Prompt {
        async fn materialize(
            &self,
            _request: &ContextRequest,
        ) -> Result<PromptMaterialization, context::ports::PromptMaterializationError> {
            Ok(PromptMaterialization {
                cacheable: vec![],
                uncached: vec![],
                revision: 0,
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
        Arc::new(context::adapters::NoOpContextMemorySource),
    );
    let request = ContextRequest {
        session_id,
        request_id: ContextRequestId::new("request-window"),
        run_id: RunId::new("active"),
        step_id: RunStepId::new("active-step"),
        pending_messages: vec![],
        invocation_reminders: vec![],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::new("en"),
        agent_roles: Default::default(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_total_tokens: None,
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
        .append_accepted_input(&accepted_input("input-v1"))
        .await
        .unwrap();
    backing
        .advance_tool_receipt(ToolReceiptMutation::pending(
            identity.clone(),
            r#"{"file_path":"/repo/src/lib.rs"}"#,
        ))
        .await
        .unwrap();
    backing
        .advance_tool_receipt(ToolReceiptMutation::terminal(
            identity,
            ToolTerminalReceipt::new(
                ToolOutcomeKind::Success,
                "terminal",
                CleanupConfirmation::NotApplicable,
            ),
        ))
        .await
        .unwrap();
    let mut outcome = append("outcome-v1");
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

    let input = accepted_input("input-v1");
    let receipt = backing.append_accepted_input(&input).await.unwrap();
    assert_eq!(receipt.committed_revision, SessionRevision::new(1));
    assert_eq!(
        backing.append_accepted_input(&input).await.unwrap(),
        receipt
    );

    let mut outcome = append("outcome-v1");
    outcome.expected_revision = SessionRevision::new(1);
    backing.append_finalized(&outcome).await.unwrap();
    assert_eq!(
        backing.snapshot(&session_id).await.unwrap().messages.len(),
        2
    );

    let mut conflict = input;
    conflict.fingerprint = ContentFingerprint::new("input-v2");
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
    let mut outcome = append("outcome-v1");
    outcome.finalize_cause = FinalizeCause::RunTerminated;
    outcome.api_input_tokens = Some(21);
    outcome.receipts = vec![context::domain::StepReceipt::agent(
        "agent-call",
        0,
        context::domain::ToolOutcomeKind::CancellationUnconfirmed,
    )];

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
    let first = backing.append_finalized(&append("same")).await.unwrap();
    let second = backing.append_finalized(&append("same")).await.unwrap();
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
    backing.append_finalized(&append("first")).await.unwrap();
    assert!(matches!(
        backing.append_finalized(&append("other")).await,
        Err(ContextAppendError::ContentConflict { .. })
    ));
}

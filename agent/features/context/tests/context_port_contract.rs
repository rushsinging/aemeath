use std::collections::HashMap;

use async_trait::async_trait;
use context::context_port::{
    AppendReceipt, CalendarDate, CompactOutcome, CompactRequest, CompactResult, CompactTrigger,
    CompactionDecision, ContentFingerprint, ContextAppend, ContextAppendError, ContextMessage,
    ContextPort, ContextPortError, ContextRequest, ContextRequestId, ContextWindow, DecisionReason,
    FinalizeCause, Language, ManualCompactRequest, RunStepId, SessionId, SessionRevision,
    StepReceipt, SystemPromptSpec, TaskReminderSnapshot, TokenBudget, ToolOutcomeKind, Urgency,
};
use provider::ReasoningLevel;
use sdk::RunId;
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::Message;

struct FakeContextPort;

fn decision() -> CompactionDecision {
    CompactionDecision {
        needed: false,
        urgency: Urgency::None,
        estimated_tokens: 12,
        threshold: 100,
        reason: DecisionReason::Heuristic,
    }
}

fn request() -> ContextRequest {
    ContextRequest {
        session_id: SessionId::new("session-1"),
        request_id: ContextRequestId::new("request-1"),
        run_id: RunId::new("run-1"),
        step_id: RunStepId::new("step-1"),
        pending_messages: vec![Message::user("hello")],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".into(),
        effective_reasoning: ReasoningLevel::Off,
        current_date: CalendarDate::new("2026-07-15"),
        task_reminder: TaskReminderSnapshot::default(),
        language: Language::new("zh"),
        agent_roles: HashMap::new(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_input_tokens: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
        prev_system_tokens: None,
        prev_tool_schema_tokens: None,
    }
}

#[async_trait]
impl ContextPort for FakeContextPort {
    async fn build_window(
        &self,
        request: &ContextRequest,
    ) -> Result<ContextWindow, ContextPortError> {
        Ok(ContextWindow {
            backing_revision: SessionRevision::new(3),
            system_blocks: vec![],
            messages: request.pending_messages.clone(),
            tool_schemas: request.tool_schemas.clone(),
            token_estimation: TokenBudget::default(),
            compaction_decision: decision(),
        })
    }

    async fn needs_compaction(
        &self,
        _request: &ContextRequest,
    ) -> Result<CompactionDecision, ContextPortError> {
        Ok(decision())
    }

    async fn compact(&self, request: &CompactRequest) -> Result<CompactOutcome, ContextPortError> {
        assert_eq!(request.trigger, CompactTrigger::Automatic);
        Ok(CompactOutcome::Committed(CompactResult {
            summary: "summary".into(),
            recent_messages: vec![],
            source_revision: SessionRevision::new(3),
        }))
    }

    async fn manual_compact(
        &self,
        request: &ManualCompactRequest,
    ) -> Result<CompactOutcome, ContextPortError> {
        Ok(CompactOutcome::Committed(CompactResult {
            summary: format!("manual summary for {}", request.session_id.as_str()),
            recent_messages: vec![],
            source_revision: SessionRevision::new(5),
        }))
    }

    async fn clear_session(&self, _session_id: &SessionId) -> Result<(), ContextPortError> {
        Ok(())
    }

    async fn append_and_persist(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError> {
        Ok(AppendReceipt {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: SessionRevision::new(4),
            fingerprint: append.fingerprint.clone(),
        })
    }
}

#[tokio::test]
async fn context_port_exposes_provider_neutral_six_method_contract() {
    let request = request();
    let port = FakeContextPort;

    let window = port.build_window(&request).await.unwrap();
    assert_eq!(window.messages.len(), 1);
    assert!(!port.needs_compaction(&request).await.unwrap().needed);
    assert!(matches!(
        port.compact(&CompactRequest {
            run_id: request.run_id.clone(),
            source_revision: SessionRevision::new(3),
            source: request.clone(),
            trigger: CompactTrigger::Automatic,
        })
        .await
        .unwrap(),
        CompactOutcome::Committed(_)
    ));

    let manual = port
        .manual_compact(&ManualCompactRequest {
            session_id: request.session_id.clone(),
            run_id: request.run_id.clone(),
            system_prompt: request.system_prompt.clone(),
            context_size: request.context_size,
        })
        .await
        .unwrap();
    assert!(matches!(manual, CompactOutcome::Committed(ref result)
        if result.source_revision == SessionRevision::new(5)));

    assert_eq!(port.clear_session(&request.session_id).await.unwrap(), ());
}

#[test]
fn finalized_step_supports_all_three_causes() {
    assert_eq!(
        [
            FinalizeCause::Completed,
            FinalizeCause::UserCancelledStep,
            FinalizeCause::RunTerminated,
        ]
        .len(),
        3
    );
}

fn finalized_append() -> ContextAppend {
    ContextAppend {
        session_id: SessionId::new("session-1"),
        expected_revision: SessionRevision::new(3),
        run_id: RunId::new("run-1"),
        step_id: RunStepId::new("step-1"),
        source_request_id: ContextRequestId::new("request-1"),
        finalize_cause: FinalizeCause::UserCancelledStep,
        messages: vec![Message::user("finalized")],
        receipts: vec![
            StepReceipt::tool("call-1", 0, ToolOutcomeKind::Success),
            StepReceipt::tool("call-2", 1, ToolOutcomeKind::Failure),
            StepReceipt::agent("call-3", 2, ToolOutcomeKind::Success)
                .with_summary("child finished")
                .with_artifact_ref("artifact://child-result"),
            StepReceipt::agent("call-4", 3, ToolOutcomeKind::CancellationUnconfirmed)
                .with_possible_side_effect("remote write may have started")
                .with_unfinished_call("child-call-9"),
        ],
        api_input_tokens: Some(42),
        fingerprint: ContentFingerprint::new("fingerprint-1"),
    }
}

#[test]
fn mixed_tool_outcomes_and_agent_receipts_preserve_original_order() {
    let append = finalized_append();
    let indexes: Vec<_> = append.receipts.iter().map(StepReceipt::index).collect();

    assert_eq!(indexes, vec![0, 1, 2, 3]);
    assert_eq!(append.receipts[2].summary(), Some("child finished"));
    assert_eq!(
        append.receipts[2].artifact_refs(),
        &["artifact://child-result".to_string()]
    );
    assert_eq!(
        append.receipts[3].outcome(),
        ToolOutcomeKind::CancellationUnconfirmed
    );
    assert_eq!(append.receipts[3].unfinished_call_ids(), &["child-call-9"]);
}

#[tokio::test]
async fn append_returns_typed_receipt_and_conflict_errors_remain_distinct() {
    let append = finalized_append();
    let receipt = FakeContextPort.append_and_persist(&append).await.unwrap();
    assert_eq!(receipt.committed_revision, SessionRevision::new(4));
    assert_eq!(receipt.fingerprint, append.fingerprint);

    let revision_conflict = ContextAppendError::RevisionConflict {
        expected: SessionRevision::new(3),
        actual: SessionRevision::new(4),
    };
    let content_conflict = ContextAppendError::ContentConflict {
        run_id: append.run_id,
        step_id: append.step_id,
    };
    assert!(matches!(
        revision_conflict,
        ContextAppendError::RevisionConflict { .. }
    ));
    assert!(matches!(
        content_conflict,
        ContextAppendError::ContentConflict { .. }
    ));
}

#[test]
fn published_contract_uses_context_message_not_provider_wire_content() {
    fn accepts_context_messages(_: Vec<ContextMessage>) {}
    accepts_context_messages(vec![Message::user("provider-neutral")]);
}

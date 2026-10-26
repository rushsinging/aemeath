use share::ids::InteractionRequestId;

use crate::compact::CompactResult;
use crate::context_port::{
    AcceptedInputAppendData, AcceptedInputReceiptData, AppendReceiptData, CompactOutcome,
    CompactRequestData, CompactTrigger, CompactionDecisionData, ContextAppendData,
    ContextAppendError, ContextMessage, ContextPortError, ContextRequestData, ContextWindowData,
    DecisionReason, FinalizeCause, SessionRevision, StepReceiptData, TokenBudget,
    ToolOutcomeKindData, Urgency,
};
use crate::context_port::{ContextAppendPort, ContextCompactPort, ContextWindowPort};
use async_trait::async_trait;
use sdk::RunId;
use share::ids::{RunStepId, SessionId};
use share::message::Message;

struct FakeContextPort;

fn decision() -> CompactionDecisionData {
    CompactionDecisionData {
        needed: false,
        urgency: Urgency::None,
        decision_token_count: 12,
        threshold: 100,
        context_size: 200_000,
        effective_window: 180_000,
        reason: DecisionReason::HeuristicFallback,
    }
}

#[async_trait]
impl ContextWindowPort for FakeContextPort {
    async fn build_window(
        &self,
        request: &ContextRequestData,
    ) -> Result<ContextWindowData, ContextPortError> {
        Ok(ContextWindowData {
            backing_revision: SessionRevision::new(3),
            system_blocks: vec![],
            messages: request.pending_messages.clone().into(),
            tool_schemas: request.tool_schemas.clone(),
            token_estimation: TokenBudget::default(),
            compaction_decision: decision(),
        })
    }
}

#[async_trait]
impl ContextCompactPort for FakeContextPort {
    async fn needs_compaction(
        &self,
        _request: &ContextRequestData,
    ) -> Result<CompactionDecisionData, ContextPortError> {
        Ok(decision())
    }

    async fn compact(
        &self,
        request: &CompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError> {
        assert_eq!(request.trigger, CompactTrigger::Automatic);
        Ok(CompactOutcome::Committed(CompactResult {
            summary: "summary".into(),
            recent_messages: vec![],
            source_revision: SessionRevision::new(3),
            quality: crate::domain::CompactSummaryQuality::LocalOnly,
        }))
    }
}

#[async_trait]
impl ContextAppendPort for FakeContextPort {
    async fn clear_session(&self, _session_id: &SessionId) -> Result<(), ContextPortError> {
        Ok(())
    }

    async fn append_accepted_input(
        &self,
        append: &AcceptedInputAppendData,
    ) -> Result<AcceptedInputReceiptData, crate::context_port::AcceptedInputError> {
        Ok(AcceptedInputReceiptData {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: SessionRevision::new(4),
        })
    }

    async fn append_and_persist(
        &self,
        append: &ContextAppendData,
    ) -> Result<AppendReceiptData, ContextAppendError> {
        Ok(AppendReceiptData {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: SessionRevision::new(4),
        })
    }
}

#[tokio::test]
async fn accepted_input_port_returns_typed_receipt() {
    let port = FakeContextPort;
    let accepted = AcceptedInputAppendData {
        session_id: SessionId::new("session-1"),
        run_id: RunId::new("run-1"),
        step_id: RunStepId::new("step-1"),
        source_request_id: InteractionRequestId::new("request-1"),
        messages: vec![Message::user("accepted")],
    };

    let receipt: AcceptedInputReceiptData = port.append_accepted_input(&accepted).await.unwrap();
    assert_eq!(receipt.run_id, accepted.run_id);
    assert_eq!(receipt.step_id, accepted.step_id);
    assert_eq!(receipt.committed_revision, SessionRevision::new(4));
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

fn finalized_append() -> ContextAppendData {
    ContextAppendData {
        session_id: SessionId::new("session-1"),
        expected_revision: SessionRevision::new(3),
        run_id: RunId::new("run-1"),
        step_id: RunStepId::new("step-1"),
        source_request_id: InteractionRequestId::new("request-1"),
        finalize_cause: FinalizeCause::UserCancelledStep,
        duration_ms: Some(7_325_000),
        messages: vec![Message::user("finalized")],
        receipts: vec![
            StepReceiptData {
                call_id: "call-1".to_string(),
                index: 0,
                outcome: ToolOutcomeKindData::Success,
                agent: false,
                summary: None,
                artifact_refs: Vec::new(),
                possible_side_effects: Vec::new(),
                unfinished_call_ids: Vec::new(),
            },
            StepReceiptData {
                call_id: "call-2".to_string(),
                index: 1,
                outcome: ToolOutcomeKindData::Failure,
                agent: false,
                summary: None,
                artifact_refs: Vec::new(),
                possible_side_effects: Vec::new(),
                unfinished_call_ids: Vec::new(),
            },
            StepReceiptData {
                call_id: "call-3".to_string(),
                index: 2,
                outcome: ToolOutcomeKindData::Success,
                agent: true,
                summary: Some("child finished".to_string()),
                artifact_refs: vec!["artifact://child-result".to_string()],
                possible_side_effects: Vec::new(),
                unfinished_call_ids: Vec::new(),
            },
            StepReceiptData {
                call_id: "call-4".to_string(),
                index: 3,
                outcome: ToolOutcomeKindData::CancellationUnconfirmed,
                agent: true,
                summary: None,
                artifact_refs: Vec::new(),
                possible_side_effects: vec!["remote write may have started".to_string()],
                unfinished_call_ids: vec!["child-call-9".to_string()],
            },
        ],
        api_input_tokens: Some(42),
    }
}

#[test]
fn mixed_tool_outcomes_and_agent_receipts_preserve_original_order() {
    let append = finalized_append();
    let indexes: Vec<_> = append.receipts.iter().map(|r| r.index).collect();

    assert_eq!(indexes, vec![0, 1, 2, 3]);
    assert_eq!(
        append.receipts[2].summary.as_deref(),
        Some("child finished")
    );
    assert_eq!(
        &append.receipts[2].artifact_refs[..],
        &["artifact://child-result".to_string()]
    );
    assert_eq!(
        append.receipts[3].outcome,
        ToolOutcomeKindData::CancellationUnconfirmed
    );
    assert_eq!(
        &append.receipts[3].unfinished_call_ids[..],
        &["child-call-9"]
    );
}

#[tokio::test]
async fn append_returns_typed_receipt_and_conflict_errors_remain_distinct() {
    let append = finalized_append();
    let receipt = FakeContextPort.append_and_persist(&append).await.unwrap();
    assert_eq!(receipt.committed_revision, SessionRevision::new(4));

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

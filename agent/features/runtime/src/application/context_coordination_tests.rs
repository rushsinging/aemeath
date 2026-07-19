use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::ports::{
    AppendReceipt, CalendarDate, CompactOutcome, CompactRequest, CompactResult, CompactSkipReason,
    CompactionDecision, ContentFingerprint, ContextAppend, ContextAppendError, ContextPort,
    ContextPortError, ContextRequest, ContextRequestId, ContextWindow, DecisionReason,
    FinalizeCause, Language, ManualCompactRequest, SessionId, SessionRevision, StepReceipt,
    SystemBlock, SystemPromptSpec, TaskReminderSnapshot, TokenBudget, ToolOutcomeKind, Urgency,
};
use async_trait::async_trait;
use provider::ReasoningLevel;
use sdk::{RunId, RunStepId};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::{ContentBlock, Message, Role};

use super::ContextCoordinator;

#[derive(Default)]
struct RecordingPort {
    calls: Mutex<Vec<&'static str>>,
    appends: Mutex<Vec<ContextAppend>>,
    compact_requests: Mutex<Vec<CompactRequest>>,
}

#[async_trait]
impl ContextPort for RecordingPort {
    async fn build_window(
        &self,
        request: &ContextRequest,
    ) -> Result<ContextWindow, ContextPortError> {
        self.calls.lock().unwrap().push("build_window");
        Ok(ContextWindow {
            backing_revision: SessionRevision::new(1),
            system_blocks: vec![SystemBlock {
                kind: "system_prompt".to_string(),
                content: "system".to_string(),
                cacheable: true,
            }],
            messages: request.pending_messages.clone(),
            tool_schemas: request.tool_schemas.clone(),
            token_estimation: TokenBudget {
                system_tokens: 2,
                tool_schema_tokens: 3,
                message_tokens: 5,
                total_tokens: 10,
            },
            compaction_decision: CompactionDecision {
                needed: false,
                urgency: Urgency::None,
                estimated_tokens: 0,
                threshold: 1,
                reason: DecisionReason::Heuristic,
            },
        })
    }

    async fn needs_compaction(
        &self,
        _request: &ContextRequest,
    ) -> Result<CompactionDecision, ContextPortError> {
        self.calls.lock().unwrap().push("needs_compaction");
        Ok(CompactionDecision {
            needed: true,
            urgency: Urgency::Must,
            estimated_tokens: 100,
            threshold: 90,
            reason: DecisionReason::ActualApiWithDelta,
        })
    }

    async fn compact(&self, request: &CompactRequest) -> Result<CompactOutcome, ContextPortError> {
        self.calls.lock().unwrap().push("compact");
        self.compact_requests.lock().unwrap().push(request.clone());
        Ok(CompactOutcome::Committed(CompactResult {
            summary: "summary".to_string(),
            recent_messages: request.source.pending_messages.clone(),
            source_revision: SessionRevision::new(1),
        }))
    }

    async fn manual_compact(
        &self,
        request: &ManualCompactRequest,
    ) -> Result<CompactOutcome, ContextPortError> {
        self.calls.lock().unwrap().push("manual_compact");
        Ok(CompactOutcome::Committed(CompactResult {
            summary: format!("manual summary for {}", request.session_id.as_str()),
            recent_messages: vec![],
            source_revision: SessionRevision::new(2),
        }))
    }

    async fn clear_session(&self, session_id: &SessionId) -> Result<(), ContextPortError> {
        self.calls.lock().unwrap().push("clear_session");
        assert!(!session_id.as_str().is_empty());
        Ok(())
    }

    async fn append_and_persist(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError> {
        self.calls.lock().unwrap().push("append_and_persist");
        self.appends.lock().unwrap().push(append.clone());
        Ok(AppendReceipt {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: SessionRevision::new(2),
            fingerprint: append.fingerprint.clone(),
        })
    }
}

fn request() -> ContextRequest {
    ContextRequest {
        session_id: SessionId::new("session"),
        request_id: ContextRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![Message::user("input")],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".to_string(),
        effective_reasoning: ReasoningLevel::Off,
        current_date: CalendarDate::new("2026-07-19"),
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

#[tokio::test]
async fn coordinator_uses_same_frozen_request_for_build_decision_and_compact() {
    let port = Arc::new(RecordingPort::default());
    let coordinator = ContextCoordinator::new(port.clone());
    let frozen = request();

    coordinator.build_window(&frozen).await.unwrap();
    coordinator.needs_compaction(&frozen).await.unwrap();
    coordinator
        .compact(&frozen, SessionRevision::new(1))
        .await
        .unwrap();

    assert_eq!(
        *port.calls.lock().unwrap(),
        vec!["build_window", "needs_compaction", "compact"]
    );
    let compact_requests = port.compact_requests.lock().unwrap();
    assert_eq!(compact_requests.len(), 1);
    assert_eq!(compact_requests[0].source_revision, SessionRevision::new(1));
    assert_eq!(compact_requests[0].source.request_id, frozen.request_id);
    assert_eq!(compact_requests[0].source.step_id, frozen.step_id);
}

#[tokio::test]
async fn coordinator_returns_complete_window_and_decision_fields() {
    let port = Arc::new(RecordingPort::default());
    let coordinator = ContextCoordinator::new(port);
    let frozen = request();

    let window = coordinator.build_window(&frozen).await.unwrap();
    assert_eq!(window.backing_revision, SessionRevision::new(1));
    assert_eq!(window.system_blocks.len(), 1);
    assert_eq!(window.system_blocks[0].kind, "system_prompt");
    assert_eq!(
        serde_json::to_value(&window.messages).unwrap(),
        serde_json::to_value(&frozen.pending_messages).unwrap()
    );
    assert_eq!(window.token_estimation.total_tokens, 10);
    assert_eq!(window.compaction_decision.reason, DecisionReason::Heuristic);
    assert!(coordinator.needs_compaction(&frozen).await.unwrap());
}
#[tokio::test]
async fn coordinator_delegates_manual_compact_and_clear_session_to_port() {
    let port = Arc::new(RecordingPort::default());
    let coordinator = ContextCoordinator::new(port.clone());
    let frozen = request();

    let manual = coordinator
        .manual_compact(&ManualCompactRequest {
            session_id: frozen.session_id.clone(),
            run_id: frozen.run_id.clone(),
            system_prompt: frozen.system_prompt.clone(),
            context_size: frozen.context_size,
        })
        .await
        .unwrap();
    assert!(matches!(
        manual,
        CompactOutcome::Committed(ref result) if result.source_revision == SessionRevision::new(2)
    ));

    coordinator.clear_session(&frozen.session_id).await.unwrap();

    assert_eq!(
        *port.calls.lock().unwrap(),
        vec!["manual_compact", "clear_session"]
    );
}

#[tokio::test]
async fn finalized_step_appends_once_with_original_message_order() {
    let port = Arc::new(RecordingPort::default());
    let coordinator = ContextCoordinator::new(port.clone());
    let frozen = request();
    let messages = vec![
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "assistant".to_string(),
            }],
            metadata: None,
        },
        Message::user("tool-result-1"),
        Message::user("tool-result-2"),
    ];

    coordinator
        .append_finalized(
            &frozen,
            RunStepId::new("step"),
            SessionRevision::new(1),
            FinalizeCause::Completed,
            messages.clone(),
            vec![],
            None,
        )
        .await
        .unwrap();

    let appends = port.appends.lock().unwrap();
    assert_eq!(appends.len(), 1);
    assert_eq!(
        serde_json::to_value(&appends[0].messages).unwrap(),
        serde_json::to_value(&messages).unwrap()
    );
    assert_eq!(
        port.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| **call == "append_and_persist")
            .count(),
        1
    );
    assert_ne!(appends[0].fingerprint, ContentFingerprint::new(""));
}

#[tokio::test]
async fn finalized_step_returns_receipt_and_preserves_every_boundary_field() {
    let port = Arc::new(RecordingPort::default());
    let coordinator = ContextCoordinator::new(port.clone());
    let frozen = request();
    let messages = vec![Message::user("finalized")];
    let receipts = vec![
        StepReceipt::tool("tool-1", 0, ToolOutcomeKind::Failure),
        StepReceipt::agent("agent-1", 1, ToolOutcomeKind::CancellationUnconfirmed)
            .with_summary("child partial")
            .with_artifact_ref("artifact://child")
            .with_possible_side_effect("remote write may have started")
            .with_unfinished_call("nested-1"),
    ];

    let receipt = coordinator
        .append_finalized(
            &frozen,
            RunStepId::new("final-step"),
            SessionRevision::new(7),
            FinalizeCause::UserCancelledStep,
            messages.clone(),
            receipts.clone(),
            Some(4_096),
        )
        .await
        .unwrap();

    let appends = port.appends.lock().unwrap();
    let append = &appends[0];
    assert_eq!(append.session_id, frozen.session_id);
    assert_eq!(append.run_id, frozen.run_id);
    assert_eq!(append.step_id, RunStepId::new("final-step"));
    assert_eq!(append.source_request_id, frozen.request_id);
    assert_eq!(append.expected_revision, SessionRevision::new(7));
    assert_eq!(append.finalize_cause, FinalizeCause::UserCancelledStep);
    assert_eq!(
        serde_json::to_value(&append.messages).unwrap(),
        serde_json::to_value(&messages).unwrap()
    );
    assert_eq!(append.receipts, receipts);
    assert_eq!(append.api_input_tokens, Some(4_096));
    assert_eq!(receipt.run_id, append.run_id);
    assert_eq!(receipt.step_id, append.step_id);
    assert_eq!(receipt.committed_revision, SessionRevision::new(2));
    assert_eq!(receipt.fingerprint, append.fingerprint);
}

#[tokio::test]
async fn fingerprint_is_stable_and_sensitive_to_finalized_facts() {
    async fn fingerprint_for(
        cause: FinalizeCause,
        messages: Vec<Message>,
        receipts: Vec<StepReceipt>,
        api_input_tokens: Option<u64>,
    ) -> ContentFingerprint {
        let port = Arc::new(RecordingPort::default());
        let coordinator = ContextCoordinator::new(port.clone());
        coordinator
            .append_finalized(
                &request(),
                RunStepId::new("step"),
                SessionRevision::new(1),
                cause,
                messages,
                receipts,
                api_input_tokens,
            )
            .await
            .unwrap();
        let fingerprint = port.appends.lock().unwrap()[0].fingerprint.clone();
        fingerprint
    }

    let base = fingerprint_for(
        FinalizeCause::Completed,
        vec![Message::user("fact")],
        vec![StepReceipt::tool("tool", 0, ToolOutcomeKind::Success)],
        Some(1),
    )
    .await;
    let same = fingerprint_for(
        FinalizeCause::Completed,
        vec![Message::user("fact")],
        vec![StepReceipt::tool("tool", 0, ToolOutcomeKind::Success)],
        Some(1),
    )
    .await;
    assert_eq!(base, same);

    for changed in [
        fingerprint_for(
            FinalizeCause::RunTerminated,
            vec![Message::user("fact")],
            vec![StepReceipt::tool("tool", 0, ToolOutcomeKind::Success)],
            Some(1),
        )
        .await,
        fingerprint_for(
            FinalizeCause::Completed,
            vec![Message::user("different")],
            vec![StepReceipt::tool("tool", 0, ToolOutcomeKind::Success)],
            Some(1),
        )
        .await,
        fingerprint_for(
            FinalizeCause::Completed,
            vec![Message::user("fact")],
            vec![StepReceipt::tool("tool", 0, ToolOutcomeKind::Failure)],
            Some(1),
        )
        .await,
        fingerprint_for(
            FinalizeCause::Completed,
            vec![Message::user("fact")],
            vec![StepReceipt::tool("tool", 0, ToolOutcomeKind::Success)],
            Some(2),
        )
        .await,
    ] {
        assert_ne!(base, changed);
    }
}

#[tokio::test]
async fn append_conflict_is_returned_without_hidden_retry() {
    struct ConflictPort {
        calls: Mutex<usize>,
    }
    #[async_trait]
    impl ContextPort for ConflictPort {
        async fn build_window(
            &self,
            _: &ContextRequest,
        ) -> Result<ContextWindow, ContextPortError> {
            unreachable!()
        }
        async fn needs_compaction(
            &self,
            _: &ContextRequest,
        ) -> Result<CompactionDecision, ContextPortError> {
            unreachable!()
        }
        async fn compact(&self, _: &CompactRequest) -> Result<CompactOutcome, ContextPortError> {
            unreachable!()
        }
        async fn manual_compact(
            &self,
            _: &ManualCompactRequest,
        ) -> Result<CompactOutcome, ContextPortError> {
            unreachable!()
        }
        async fn clear_session(&self, _: &SessionId) -> Result<(), ContextPortError> {
            unreachable!()
        }
        async fn append_and_persist(
            &self,
            _: &ContextAppend,
        ) -> Result<AppendReceipt, ContextAppendError> {
            *self.calls.lock().unwrap() += 1;
            Err(ContextAppendError::RevisionConflict {
                expected: SessionRevision::new(1),
                actual: SessionRevision::new(2),
            })
        }
    }

    let port = Arc::new(ConflictPort {
        calls: Mutex::new(0),
    });
    let coordinator = ContextCoordinator::new(port.clone());
    assert!(matches!(
        coordinator
            .append_finalized(
                &request(),
                RunStepId::new("step"),
                SessionRevision::new(1),
                FinalizeCause::Completed,
                vec![Message::user("fact")],
                vec![],
                None,
            )
            .await,
        Err(ContextAppendError::RevisionConflict { .. })
    ));
    assert_eq!(*port.calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn skipped_compaction_is_returned_without_hidden_retry() {
    struct SkippingPort;
    #[async_trait]
    impl ContextPort for SkippingPort {
        async fn build_window(
            &self,
            _: &ContextRequest,
        ) -> Result<ContextWindow, ContextPortError> {
            unreachable!()
        }
        async fn needs_compaction(
            &self,
            _: &ContextRequest,
        ) -> Result<CompactionDecision, ContextPortError> {
            unreachable!()
        }
        async fn compact(&self, _: &CompactRequest) -> Result<CompactOutcome, ContextPortError> {
            Ok(CompactOutcome::Skipped(CompactSkipReason::ResumeProtection))
        }
        async fn manual_compact(
            &self,
            _: &ManualCompactRequest,
        ) -> Result<CompactOutcome, ContextPortError> {
            unreachable!()
        }
        async fn clear_session(&self, _: &SessionId) -> Result<(), ContextPortError> {
            unreachable!()
        }
        async fn append_and_persist(
            &self,
            _: &ContextAppend,
        ) -> Result<AppendReceipt, ContextAppendError> {
            unreachable!()
        }
    }

    let coordinator = ContextCoordinator::new(Arc::new(SkippingPort));
    assert!(matches!(
        coordinator
            .compact(&request(), SessionRevision::new(0))
            .await
            .unwrap(),
        CompactOutcome::Skipped(CompactSkipReason::ResumeProtection)
    ));
}

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::ports::{
    AppendReceipt, CalendarDate, CompactOutcome, CompactRequest, CompactResult, CompactSkipReason,
    CompactionDecision, ContentFingerprint, ContextAppend, ContextAppendError, ContextPort,
    ContextPortError, ContextRequest, ContextRequestId, ContextWindow, DecisionReason,
    FinalizeCause, Language, ManualCompactRequest, SessionId, SessionRevision, SystemPromptSpec,
    TaskReminderSnapshot, TokenBudget, Urgency,
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
            system_blocks: vec![],
            messages: request.pending_messages.clone(),
            tool_schemas: request.tool_schemas.clone(),
            token_estimation: TokenBudget::default(),
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

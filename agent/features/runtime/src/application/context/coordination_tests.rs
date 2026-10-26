use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::ports::{
    AcceptedInputAppendData, AcceptedInputError, AcceptedInputReceiptData, AppendReceiptData,
    CompactOutcome, CompactRequestData, CompactionDecisionData, ContextAppendData,
    ContextAppendError, ContextPortError, ContextRequestData, ContextWindowData, DecisionReason,
    FinalizeCause, InteractionRequestId, Language, SessionId, SessionRevision, StepReceiptData,
    SystemBlock, TokenBudget, ToolOutcomeKindData, Urgency,
};
use async_trait::async_trait;
use context::compact::{CompactResult, CompactSkipReason, CompactSummaryQuality};
use context::{ContextAppendPort, ContextCompactPort, ContextWindowPort};
use sdk::{RunId, RunStepId};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::{ContentBlock, Message, Role};
use share::reasoning::ReasoningLevel;

use super::{apply_automatic_compact_outcome, ContextCoordinator};

#[derive(Default)]
struct RecordingPort {
    calls: Mutex<Vec<&'static str>>,
    appends: Mutex<Vec<ContextAppendData>>,
    accepted_inputs: Mutex<Vec<AcceptedInputAppendData>>,
    compact_requests: Mutex<Vec<CompactRequestData>>,
}

#[async_trait]
#[async_trait]
impl ContextWindowPort for RecordingPort {
    async fn build_window(
        &self,
        request: &ContextRequestData,
    ) -> Result<ContextWindowData, ContextPortError> {
        self.calls.lock().unwrap().push("build_window");
        Ok(ContextWindowData {
            backing_revision: SessionRevision::new(1),
            system_blocks: vec![SystemBlock {
                kind: "system_prompt".to_string(),
                content: "system".to_string(),
                cacheable: true,
                cache_break: true,
            }],
            messages: request.pending_messages.clone().into(),
            tool_schemas: request.tool_schemas.clone(),
            token_estimation: TokenBudget {
                system_tokens: 2,
                tool_schema_tokens: 3,
                message_tokens: 5,
                total_tokens: 10,
            },
            compaction_decision: CompactionDecisionData {
                needed: false,
                urgency: Urgency::None,
                decision_token_count: 0,
                threshold: 1,
                context_size: 200_000,
                effective_window: 180_000,
                reason: DecisionReason::HeuristicFallback,
            },
        })
    }
}
#[async_trait]
impl ContextCompactPort for RecordingPort {
    async fn needs_compaction(
        &self,
        _request: &ContextRequestData,
    ) -> Result<CompactionDecisionData, ContextPortError> {
        self.calls.lock().unwrap().push("needs_compaction");
        Ok(CompactionDecisionData {
            needed: true,
            urgency: Urgency::Must,
            decision_token_count: 100,
            threshold: 90,
            context_size: 200_000,
            effective_window: 180_000,
            reason: DecisionReason::ActualProviderUsage,
        })
    }

    async fn compact(
        &self,
        request: &CompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError> {
        self.calls.lock().unwrap().push("compact");
        self.compact_requests.lock().unwrap().push(request.clone());
        Ok(CompactOutcome::Committed(CompactResult {
            summary: "summary".to_string(),
            recent_messages: request.source.pending_messages.clone(),
            source_revision: SessionRevision::new(1),
            quality: CompactSummaryQuality::LocalOnly,
        }))
    }
}
// 与原 ContextPort 默认实现等价的 no-op reminder 面。
impl context::ReminderControlPort for RecordingPort {}

#[async_trait]
impl ContextAppendPort for RecordingPort {
    async fn clear_session(&self, session_id: &SessionId) -> Result<(), ContextPortError> {
        self.calls.lock().unwrap().push("clear_session");
        assert!(!session_id.as_str().is_empty());
        Ok(())
    }

    async fn append_accepted_input(
        &self,
        append: &AcceptedInputAppendData,
    ) -> Result<AcceptedInputReceiptData, AcceptedInputError> {
        self.calls.lock().unwrap().push("append_accepted_input");
        self.accepted_inputs.lock().unwrap().push(append.clone());
        Ok(AcceptedInputReceiptData {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: SessionRevision::new(2),
        })
    }

    async fn append_and_persist(
        &self,
        append: &ContextAppendData,
    ) -> Result<AppendReceiptData, ContextAppendError> {
        self.calls.lock().unwrap().push("append_and_persist");
        self.appends.lock().unwrap().push(append.clone());
        Ok(AppendReceiptData {
            run_id: append.run_id.clone(),
            step_id: append.step_id.clone(),
            committed_revision: SessionRevision::new(2),
        })
    }
}

fn request() -> ContextRequestData {
    ContextRequestData {
        session_id: SessionId::new("session"),
        request_id: InteractionRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![Message::user("input")],
        workspace_root: std::path::PathBuf::from("/tmp/workspace"),
        workspace_is_git: false,
        model_id: "fake/model".to_string(),
        effective_reasoning: ReasoningLevel::Off,
        language: Language::Zh,
        agent_roles: HashMap::new(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_total_tokens: None,
        heuristic_calibration: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

#[tokio::test]
async fn coordinator_uses_same_frozen_request_for_build_decision_and_compact() {
    let port = Arc::new(RecordingPort::default());
    let coordinator = ContextCoordinator::new(context::ContextPorts::from_service(port.clone()));
    let frozen = request();

    coordinator.build_window(&frozen).await.unwrap();
    coordinator.needs_compaction(&frozen).await.unwrap();
    coordinator
        .compact(
            &frozen,
            SessionRevision::new(1),
            std::sync::Arc::new(|_: sdk::CompactStageView, _: sdk::CompactWorkView| {}),
            None,
            tokio_util::sync::CancellationToken::new(),
        )
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
    let coordinator = ContextCoordinator::new(context::ContextPorts::from_service(port));
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
    assert_eq!(
        window.compaction_decision.reason,
        DecisionReason::HeuristicFallback
    );
    assert!(coordinator.needs_compaction(&frozen).await.unwrap());
}

#[tokio::test]
async fn coordinator_appends_only_accepted_input_with_stable_fingerprint() {
    let port = Arc::new(RecordingPort::default());
    let coordinator = ContextCoordinator::new(context::ContextPorts::from_service(port.clone()));
    let frozen = request();
    let messages = vec![Message::user("accepted")];

    coordinator
        .append_accepted_input(&frozen, messages.clone())
        .await
        .unwrap();

    let accepted = port.accepted_inputs.lock().unwrap();
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].run_id, frozen.run_id);
    assert_eq!(
        serde_json::to_value(&accepted[0].messages).unwrap(),
        serde_json::to_value(&messages).unwrap()
    );
}

#[tokio::test]
async fn finalized_step_appends_once_with_original_message_order() {
    let port = Arc::new(RecordingPort::default());
    let coordinator = ContextCoordinator::new(context::ContextPorts::from_service(port.clone()));
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
            None,
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
}

#[tokio::test]
async fn finalized_step_returns_receipt_and_preserves_every_boundary_field() {
    let port = Arc::new(RecordingPort::default());
    let coordinator = ContextCoordinator::new(context::ContextPorts::from_service(port.clone()));
    let frozen = request();
    let messages = vec![Message::user("finalized")];
    let receipts = vec![
        StepReceiptData::tool("tool-1", 0, ToolOutcomeKindData::Failure),
        StepReceiptData::agent("agent-1", 1, ToolOutcomeKindData::CancellationUnconfirmed)
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
            Some(7_325_000),
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
    assert_eq!(append.duration_ms, Some(7_325_000));
    assert_eq!(
        serde_json::to_value(&append.messages).unwrap(),
        serde_json::to_value(&messages).unwrap()
    );
    assert_eq!(append.receipts, receipts);
    assert_eq!(append.api_input_tokens, Some(4_096));
    assert_eq!(receipt.run_id, append.run_id);
    assert_eq!(receipt.step_id, append.step_id);
    assert_eq!(receipt.committed_revision, SessionRevision::new(2));
}

/// #1385 TaskData 12: Tests updated for RunUsageTracker.
#[test]
fn automatic_compact_committed_resets_usage_and_window() {
    let usage = crate::application::run::context::RunUsageTracker::new();
    usage.update(42);
    let mut window = Some("window");

    apply_automatic_compact_outcome(
        &CompactOutcome::Committed(CompactResult {
            summary: "summary".to_string(),
            recent_messages: Vec::new(),
            source_revision: SessionRevision::new(7),
            quality: CompactSummaryQuality::LocalOnly,
        }),
        &usage,
        &mut window,
    );

    assert_eq!(usage.get(), None);
    assert_eq!(window, None);
}

#[test]
fn automatic_compact_skipped_preserves_usage_and_window() {
    let usage = crate::application::run::context::RunUsageTracker::new();
    usage.update(42);
    let mut window = Some("window");

    apply_automatic_compact_outcome(
        &CompactOutcome::Skipped(CompactSkipReason::ResumeProtection),
        &usage,
        &mut window,
    );

    assert_eq!(usage.get(), Some(42));
    assert_eq!(window, Some("window"));
}

/// #1500：ContextCoordinator 必须把 Runtime 的进度视图透传到 CompactRequestData，
/// 且 domain stage/chunk 计数正确映射为 SDK 视图（Preparing/Summarizing
/// 带 chunk 计数/Finalizing）。
#[tokio::test]
async fn compact_progress_forwarding_reaches_request_and_maps_stage_and_chunks() {
    let port = Arc::new(RecordingPort::default());
    let coordinator = ContextCoordinator::new(context::ContextPorts::from_service(port.clone()));
    let frozen = request();

    type CompactProgressRecord = (sdk::CompactStageView, sdk::CompactWorkView);
    let received: Arc<Mutex<Vec<CompactProgressRecord>>> = Arc::new(Mutex::new(Vec::new()));
    let view = {
        let received = received.clone();
        std::sync::Arc::new(
            move |stage: sdk::CompactStageView, work: sdk::CompactWorkView| {
                received.lock().unwrap().push((stage, work));
            },
        )
    };

    coordinator
        .compact(
            &frozen,
            SessionRevision::new(1),
            view,
            None,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();

    let requests = port.compact_requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let progress = requests[0]
        .progress
        .as_ref()
        .expect("compact progress 必须接线到 CompactRequestData");

    // 触发 context 侧进度 → 视图收到映射后的 stage/chunk 计数
    progress.emit(
        context::compact::CompactStageData::Preparing,
        context::compact::CompactWorkData::Indeterminate,
    );
    progress.emit(
        context::compact::CompactStageData::Mapping,
        context::compact::CompactWorkData::Determinate {
            completed: 2,
            total: 5,
        },
    );
    progress.emit(
        context::compact::CompactStageData::Finalizing,
        context::compact::CompactWorkData::Indeterminate,
    );

    assert_eq!(
        *received.lock().unwrap(),
        vec![
            (
                sdk::CompactStageView::Preparing,
                sdk::CompactWorkView::Indeterminate,
            ),
            (
                sdk::CompactStageView::Mapping,
                sdk::CompactWorkView::Determinate {
                    completed: 2,
                    total: 5,
                },
            ),
            (
                sdk::CompactStageView::Finalizing,
                sdk::CompactWorkView::Indeterminate,
            ),
        ],
        "domain 进度必须原样映射到 SDK 视图（#1500）"
    );
}

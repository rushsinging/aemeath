use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::ports::{
    CompactOutcome, ContextRequestData, DecisionReason, FinalizeCause, InteractionRequestId,
    Language, RunStepReceiptData, SessionId, SessionRevision, ToolOutcomeKindData,
};
use context::compact::{CompactResult, CompactSkipReason, CompactSummaryQuality};
use sdk::{RunId, RunStepId};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::{ContentBlock, Message, Role};
use share::reasoning::ReasoningLevel;

use super::{apply_automatic_compact_outcome, ContextCoordinator};

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
async fn coordinator_exposes_current_revision_for_step_cas_baseline() {
    // Step 收口前刷新 CAS 基准，避免窗口快照 revision 落后于本 step
    // 自身 accepted-input / 工具回执推进后被误判成并发冲突。
    let port = Arc::new(context::test_support::StubContextService::new());
    let coordinator = ContextCoordinator::new(context::ContextPorts::from_service(port.clone()));

    let revision = coordinator
        .current_revision(&SessionId::new("session"))
        .await
        .unwrap();

    assert_eq!(revision, Some(SessionRevision::new(7)));
    assert!(port.calls().contains(&"current_revision"));
}

#[tokio::test]
async fn coordinator_uses_same_frozen_request_for_build_decision_and_compact() {
    let port = Arc::new(context::test_support::StubContextService::new());
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
        port.calls(),
        vec!["build_window", "needs_compaction", "compact"]
    );
    let compact_requests = port.compact_requests();
    assert_eq!(compact_requests.len(), 1);
    assert_eq!(compact_requests[0].source_revision, SessionRevision::new(1));
    assert_eq!(compact_requests[0].source.request_id, frozen.request_id);
    assert_eq!(compact_requests[0].source.step_id, frozen.step_id);
}

#[tokio::test]
async fn coordinator_returns_complete_window_and_decision_fields() {
    let port = Arc::new(context::test_support::StubContextService::new());
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
    let port = Arc::new(context::test_support::StubContextService::new());
    let coordinator = ContextCoordinator::new(context::ContextPorts::from_service(port.clone()));
    let frozen = request();
    let messages = vec![Message::user("accepted")];

    coordinator
        .append_accepted_input(&frozen, messages.clone())
        .await
        .unwrap();

    let accepted = port.accepted_inputs();
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].run_id, frozen.run_id);
    assert_eq!(
        serde_json::to_value(&accepted[0].messages).unwrap(),
        serde_json::to_value(&messages).unwrap()
    );
}

#[tokio::test]
async fn finalized_step_appends_once_with_original_message_order() {
    let port = Arc::new(context::test_support::StubContextService::new());
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

    let appends = port.append_and_persist_inputs();
    assert_eq!(appends.len(), 1);
    assert_eq!(
        serde_json::to_value(&appends[0].messages).unwrap(),
        serde_json::to_value(&messages).unwrap()
    );
    assert_eq!(
        port.calls()
            .iter()
            .filter(|call| **call == "append_and_persist")
            .count(),
        1
    );
}

#[tokio::test]
async fn finalized_step_returns_receipt_and_preserves_every_boundary_field() {
    let port = Arc::new(context::test_support::StubContextService::new());
    let coordinator = ContextCoordinator::new(context::ContextPorts::from_service(port.clone()));
    let frozen = request();
    let messages = vec![Message::user("finalized")];
    let receipts = vec![
        RunStepReceiptData {
            call_id: "tool-1".to_string(),
            index: 0,
            outcome: ToolOutcomeKindData::Failure,
            agent: false,
            summary: None,
            cleanup: tools::CleanupConfirmation::NotApplicable,
            possible_side_effects: Vec::new(),
            unfinished_call_ids: Vec::new(),
        },
        RunStepReceiptData {
            call_id: "agent-1".to_string(),
            index: 1,
            outcome: ToolOutcomeKindData::CancellationUnconfirmed,
            agent: true,
            summary: Some("child partial".to_string()),
            cleanup: tools::CleanupConfirmation::NotApplicable,
            possible_side_effects: vec!["remote write may have started".to_string()],
            unfinished_call_ids: vec!["nested-1".to_string()],
        },
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

    let appends = port.append_and_persist_inputs();
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
    let port = Arc::new(context::test_support::StubContextService::new());
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

    let requests = port.compact_requests();
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

// ─── #2142：compact 请求携带 LLM 绑定（resolver 装配时） ───

use crate::application::client::compact_model::compact_model_tests as compact_support;

fn resolver() -> std::sync::Arc<crate::application::client::CompactModelResolver> {
    let snapshot = compact_support::snapshot_with(None);
    let slot = compact_support::bound_slot(&snapshot);
    let (resolver, _builds, _reader) = compact_support::resolver(snapshot, slot);
    std::sync::Arc::new(resolver)
}

fn noop_progress() -> std::sync::Arc<dyn crate::application::loop_engine::CompactProgressView> {
    std::sync::Arc::new(|_: sdk::CompactStageView, _: sdk::CompactWorkView| {})
}

#[tokio::test]
async fn automatic_compact_carries_llm_binding_when_resolver_bound() {
    let port = Arc::new(context::test_support::StubContextService::new());
    let coordinator = ContextCoordinator::new(context::ContextPorts::from_service(port.clone()))
        .with_compact_resolver(Some(resolver()));

    coordinator
        .compact(
            &request(),
            SessionRevision::new(1),
            noop_progress(),
            None,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();

    let compact_requests = port.compact_requests();
    assert_eq!(compact_requests.len(), 1);
    let llm = compact_requests[0]
        .llm
        .as_ref()
        .expect("装配 resolver 后自动压缩请求必须携带 LLM 绑定");
    // 未配置 compact_model → 跟随会话模型（fake 会话模型 = session-model）。
    assert_eq!(llm.model.model, "session-model");
}

#[tokio::test]
async fn manual_compact_carries_llm_binding_when_resolver_bound() {
    let port = Arc::new(context::test_support::StubContextService::new());
    let coordinator = ContextCoordinator::new(context::ContextPorts::from_service(port.clone()))
        .with_compact_resolver(Some(resolver()));

    coordinator
        .manual_compact(&request(), noop_progress(), None)
        .await
        .unwrap();

    let compact_requests = port.compact_requests();
    assert_eq!(compact_requests.len(), 1);
    assert!(
        compact_requests[0].llm.is_some(),
        "手动压缩与自动压缩共用同一 resolver 单一入口"
    );
}

#[tokio::test]
async fn compact_stays_local_when_resolver_absent() {
    let port = Arc::new(context::test_support::StubContextService::new());
    let coordinator = ContextCoordinator::new(context::ContextPorts::from_service(port.clone()));

    coordinator
        .compact(
            &request(),
            SessionRevision::new(1),
            noop_progress(),
            None,
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();

    let compact_requests = port.compact_requests();
    assert_eq!(compact_requests.len(), 1);
    assert!(
        compact_requests[0].llm.is_none(),
        "未装配 resolver（isolated/测试装配）时必须保持本地压缩"
    );
}

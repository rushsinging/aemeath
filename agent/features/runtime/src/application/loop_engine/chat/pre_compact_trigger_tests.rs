//! External tests for the production PreCompact reflection trigger (#1284).
//!
//! These tests verify that the production automatic compact path
//! (`RuntimeCompaction` + `ChatCompactionObserver`) submits a
//! `ReflectionTaskTrigger::PreCompact` job using the **pre-compact** messages
//! snapshot only when the context port returns `CompactOutcome::Committed`.
//! Errors and `CompactOutcome::Skipped` must never enqueue a job. The
//! submission shares the session-scoped `ReflectionTaskAdapter` slot with
//! `Interval` and `Manual` triggers; the single-slot contention contract itself
//! is already covered by the `task_adapter_tests` in the reflection runner.

#![allow(clippy::type_complexity)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use sdk::{RunId, RunStepId};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::Message;
use tokio_util::sync::CancellationToken;

use super::main_run_port::ChatCompactionObserver;
use crate::application::loop_engine::chat::reflection::{
    maybe_submit_pre_compact_reflection, submit_pre_compact_reflection,
};
use crate::application::loop_engine::CompactionPort;
use crate::application::reflection::{
    ReflectionTaskAdapter, ReflectionTaskRequest, ReflectionTaskSubmitOutcome,
    ReflectionTaskTrigger,
};
use crate::ports::{
    CompactOutcome, CompactRequest, CompactResult, CompactSkipReason, CompactionDecision,
    ContextPort, ContextPortError, ContextRequest, ContextRequestId, ContextWindow, DecisionReason,
    Language as ContextLanguage, SessionId, SessionRevision, SystemBlock, SystemPromptSpec,
    TokenBudget, Urgency,
};

/// `submit_complete` builds its own executor closure and ignores the
/// adapter's `executor` field, so we cannot use a capturing closure to
/// observe submissions. The unit tests below therefore exercise the
/// helpers via the production adapter and a real provider whose response
/// parses as an empty reflection output. The integration tests exercise the
/// production `RuntimeCompaction` and `ChatCompactionObserver` seam, then use
/// `adapter.drain()` to join the spawned task and inspect its trigger.
fn production_adapter() -> ReflectionTaskAdapter {
    ReflectionTaskAdapter::production(Duration::from_secs(5))
}

fn frozen_request() -> ContextRequest {
    ContextRequest {
        session_id: SessionId::new("session"),
        request_id: ContextRequestId::new("request"),
        run_id: RunId::new("run"),
        step_id: RunStepId::new("step"),
        pending_messages: vec![Message::user("seed")],
        invocation_reminders: vec![],
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".to_string(),
        effective_reasoning: provider::ReasoningLevel::Off,
        language: ContextLanguage::new("en"),
        agent_roles: HashMap::new(),
        config_snapshot: ConfigSnapshot::new(Config::default()),
        context_size: 128_000,
        max_output_tokens: 8_192,
        last_api_total_tokens: None,
        tool_schemas: vec![],
        tool_schema_tokens: 0,
    }
}

fn window_with(messages: Vec<Message>) -> ContextWindow {
    ContextWindow {
        backing_revision: SessionRevision::new(7),
        system_blocks: vec![SystemBlock {
            kind: "system_prompt".to_string(),
            content: "system".to_string(),
            cacheable: true,
            cache_break: true,
        }],
        messages: messages.into(),
        tool_schemas: vec![],
        token_estimation: TokenBudget::default(),
        compaction_decision: CompactionDecision {
            needed: true,
            urgency: Urgency::Must,
            decision_token_count: 0,
            threshold: 0,
            context_size: 200_000,
            effective_window: 180_000,
            reason: DecisionReason::HeuristicFallback,
        },
    }
}

/// `ContextPort` that records compact invocations and returns a configurable
/// outcome. Other methods are no-ops because the production compact path only
/// touches `compact`.
struct StubContextPort {
    outcome: Mutex<Option<Result<CompactOutcome, ContextPortError>>>,
    compact_calls: Mutex<Vec<CompactRequest>>,
}

impl StubContextPort {
    fn new(outcome: Result<CompactOutcome, ContextPortError>) -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(Some(outcome)),
            compact_calls: Mutex::new(Vec::new()),
        })
    }

    fn compact_calls(&self) -> Vec<CompactRequest> {
        self.compact_calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl ContextPort for StubContextPort {
    async fn build_window(
        &self,
        _request: &ContextRequest,
    ) -> Result<ContextWindow, ContextPortError> {
        Err(ContextPortError::Compact("stub: build_window".to_string()))
    }

    async fn needs_compaction(
        &self,
        _request: &ContextRequest,
    ) -> Result<CompactionDecision, ContextPortError> {
        Err(ContextPortError::Compact(
            "stub: needs_compaction".to_string(),
        ))
    }

    async fn compact(&self, request: &CompactRequest) -> Result<CompactOutcome, ContextPortError> {
        self.compact_calls.lock().unwrap().push(request.clone());
        self.outcome
            .lock()
            .unwrap()
            .take()
            .expect("stub outcome must be configured exactly once")
    }

    async fn manual_compact(
        &self,
        _request: &crate::ports::ManualCompactRequest,
    ) -> Result<CompactOutcome, ContextPortError> {
        Err(ContextPortError::Compact(
            "stub: manual_compact".to_string(),
        ))
    }

    async fn clear_session(&self, _session_id: &SessionId) -> Result<(), ContextPortError> {
        Err(ContextPortError::Compact("stub: clear_session".to_string()))
    }

    async fn append_and_persist(
        &self,
        _append: &crate::ports::ContextAppend,
    ) -> Result<crate::ports::AppendReceipt, crate::ports::ContextAppendError> {
        Err(crate::ports::ContextAppendError::Storage(
            "stub".to_string(),
        ))
    }
}

fn noop_reflection_history() -> Arc<dyn memory::api::ReflectionHistoryStore> {
    struct NoopHistory;
    #[async_trait]
    impl memory::api::ReflectionHistoryQuery for NoopHistory {
        async fn list(
            &self,
            _limit: usize,
        ) -> Result<Vec<memory::api::ReflectionSafeSummary>, memory::api::MemoryError> {
            Ok(Vec::new())
        }
    }
    #[async_trait]
    impl memory::api::ReflectionHistoryStore for NoopHistory {
        async fn append(
            &self,
            _record: &memory::api::ReflectionRecord,
        ) -> Result<(), memory::api::MemoryError> {
            Ok(())
        }
        async fn upsert(
            &self,
            _record: &memory::api::ReflectionRecord,
        ) -> Result<(), memory::api::MemoryError> {
            Ok(())
        }
    }
    Arc::new(NoopHistory)
}

fn failing_append_reflection_history() -> Arc<dyn memory::api::ReflectionHistoryStore> {
    struct FailingAppendHistory;
    #[async_trait]
    impl memory::api::ReflectionHistoryQuery for FailingAppendHistory {
        async fn list(
            &self,
            _limit: usize,
        ) -> Result<Vec<memory::api::ReflectionSafeSummary>, memory::api::MemoryError> {
            Ok(Vec::new())
        }
    }
    #[async_trait]
    impl memory::api::ReflectionHistoryStore for FailingAppendHistory {
        async fn append(
            &self,
            _record: &memory::api::ReflectionRecord,
        ) -> Result<(), memory::api::MemoryError> {
            Err(memory::api::MemoryError::InvalidEntry {
                message: "history append failed".to_string(),
            })
        }
        async fn upsert(
            &self,
            _record: &memory::api::ReflectionRecord,
        ) -> Result<(), memory::api::MemoryError> {
            panic!("append failure must prevent terminal upsert")
        }
    }
    Arc::new(FailingAppendHistory)
}

/// Inline builder for the production compaction seam.
fn build_compact_test_port(
    harness: &CompactHarness,
) -> crate::application::loop_engine::run_services::RuntimeCompaction<'_, ChatCompactionObserver> {
    assert!(Arc::ptr_eq(
        &harness.runtime_context.context(),
        &harness.context_port
    ));
    crate::application::loop_engine::run_services::RuntimeCompaction::new(
        &harness.runtime_context,
        ChatCompactionObserver {
            runtime_context: harness.runtime_context.clone(),
            reflection_tasks: harness.adapter.clone(),
            system_prompt: "system".to_string(),
            language: "en".to_string(),
        },
    )
}

/// Per-test harness for the production compaction service and observer.
struct CompactHarness {
    adapter: ReflectionTaskAdapter,
    stub: Arc<StubContextPort>,
    runtime_context: crate::application::run::context::RuntimeContext,
    context_port: Arc<dyn ContextPort>,
}

impl CompactHarness {
    fn new(outcome: Result<CompactOutcome, ContextPortError>) -> Self {
        let adapter = production_adapter();
        let stub = StubContextPort::new(outcome);
        let binding = pre_compact_test_binding();
        let config_snapshot = ConfigSnapshot::new(Config::default());
        let runtime_context =
            crate::application::run::run_factory_support::SessionRunFixture::builder()
                .with_context_port(stub.clone())
                .with_provider_binding(binding)
                .with_config(config_snapshot)
                .with_session_id("session".to_string())
                .build()
                .create(crate::domain::agent_run::RunSpec::main())
                .expect("pre-compact parent run creation must succeed")
                .context()
                .clone();
        let context_port = runtime_context.context();
        Self {
            adapter,
            stub,
            runtime_context,
            context_port,
        }
    }
}

/// A test-only `ProviderPort` whose `invoke` always returns a valid reflection
/// JSON so `submit_complete` can complete end-to-end and reach the slot.
struct StaticReflectionProvider;

#[async_trait]
impl crate::ports::ProviderPort for StaticReflectionProvider {
    fn capabilities(
        &self,
        model: &provider::ModelId,
    ) -> Result<
        crate::ports::provider_port::ModelCapability,
        crate::ports::provider_port::ProviderError,
    > {
        use crate::ports::provider_port::{
            ModelCapability, ProviderError, ProviderErrorKind, ReasoningCapability,
        };
        if model.provider == "pre-compact-test" {
            Ok(ModelCapability {
                model: model.clone(),
                supports_tools: false,
                supports_parallel_tool_calls: false,
                supports_streaming: true,
                reasoning: ReasoningCapability::none(),
                context_limit: Some(128_000),
                output_limit: Some(8_192),
            })
        } else {
            Err(ProviderError::fatal(
                ProviderErrorKind::ModelUnavailable,
                format!("unknown model: {model}"),
            ))
        }
    }

    async fn invoke(
        &self,
        _request: crate::ports::provider_port::InvocationRequest,
        _cancel: &dyn crate::ports::provider_port::CancellationSignal,
    ) -> Result<
        crate::ports::provider_port::InvocationStream,
        crate::ports::provider_port::ProviderError,
    > {
        Ok(
            crate::application::model::test_support::text_completion_stream(
                r#"{"deviations":[],"suggested_memories":[],"outdated_memories":[]}"#,
                1,
                1,
            ),
        )
    }
}

/// Build a `ProviderBinding` whose provider returns a parseable reflection
/// response so `submit_complete` can drain the adapter to a terminal state.
fn pre_compact_test_binding() -> Arc<crate::ports::ProviderBinding> {
    let model = provider::ModelId {
        provider: "pre-compact-test".to_string(),
        model: "pre-compact-test-model".to_string(),
    };
    Arc::new(crate::ports::ProviderBinding {
        provider: Arc::new(StaticReflectionProvider),
        model,
        max_tokens: 8_192,
        requested_reasoning: provider::ReasoningLevel::Off,
        context_window: Some(128_000),
    })
}

/// Unit-level assertion: when `maybe_submit_pre_compact_reflection` sees
/// `Committed`, the production adapter receives exactly one PreCompact job.
/// We verify the trigger via `adapter.drain()` because `submit_complete`
/// writes a `ReflectionTaskCompletion` carrying the trigger after the
/// spawned executor settles.
#[tokio::test]
async fn maybe_submit_pre_compact_reflection_only_submits_on_committed() {
    let adapter = production_adapter();
    let binding = pre_compact_test_binding();
    let memory_config = share::config::MemoryConfig::default();
    let memory: Arc<dyn memory::MemoryPort> = Arc::new(memory::NoOpMemory);
    let history = noop_reflection_history();
    let snapshot = vec![
        Message::user("kept-by-compact"),
        Message::user("discarded-by-compact"),
    ];

    let committed = CompactOutcome::Committed(CompactResult {
        summary: "summary".to_string(),
        recent_messages: vec![],
        source_revision: SessionRevision::new(7),
        quality: context::domain::CompactSummaryQuality::LocalOnly,
    });
    let skipped = CompactOutcome::Skipped(CompactSkipReason::ResumeProtection);

    let outcome_committed = maybe_submit_pre_compact_reflection(
        &committed,
        &snapshot,
        &adapter,
        &memory_config,
        &binding,
        "system",
        "en",
        &memory,
        &history,
    );
    assert_eq!(
        outcome_committed,
        Some(ReflectionTaskSubmitOutcome::Accepted)
    );

    // Spawned task: write `Running`, call LLM, parse, upsert terminal record.
    // The completion slot will record the trigger regardless of execution
    // status (Succeeded or Failed both carry the trigger).
    let completions_committed = adapter.drain().await;
    assert_eq!(completions_committed.len(), 1, "exactly one PreCompact job");
    assert_eq!(
        completions_committed[0].trigger,
        ReflectionTaskTrigger::PreCompact,
        "Committed must enqueue a PreCompact trigger"
    );

    // Skipped → no submission. We reuse the same adapter, which has been
    // fully drained above; calling again with `Skipped` must leave the slot
    // idle and never enqueue.
    let outcome_skipped = maybe_submit_pre_compact_reflection(
        &skipped,
        &snapshot,
        &adapter,
        &memory_config,
        &binding,
        "system",
        "en",
        &memory,
        &history,
    );
    assert!(
        outcome_skipped.is_none(),
        "Skipped must report that no PreCompact job was enqueued"
    );
    let completions_skipped = adapter.drain().await;
    assert!(
        completions_skipped.is_empty(),
        "Skipped must not enqueue any job: {completions_skipped:?}"
    );
}

/// Unit-level assertion: `submit_pre_compact_reflection` (the production
/// helper) enqueues a `PreCompact` request against the production adapter.
#[tokio::test]
async fn submit_pre_compact_reflection_enqueues_precompact_request() {
    let adapter = production_adapter();
    let binding = pre_compact_test_binding();
    let memory_config = share::config::MemoryConfig::default();
    let memory: Arc<dyn memory::MemoryPort> = Arc::new(memory::NoOpMemory);
    let history = noop_reflection_history();
    let snapshot = vec![
        Message::user("alpha"),
        Message::user("beta"),
        Message::user("gamma"),
    ];

    let outcome = submit_pre_compact_reflection(
        &adapter,
        &memory_config,
        &snapshot,
        &binding,
        "system prompt text",
        "en",
        &memory,
        &history,
    );

    assert_eq!(outcome, ReflectionTaskSubmitOutcome::Accepted);
    let completions = adapter.drain().await;
    assert_eq!(completions.len(), 1);
    assert_eq!(
        completions[0].trigger,
        ReflectionTaskTrigger::PreCompact,
        "submit_pre_compact_reflection must enqueue a PreCompact job"
    );
}

#[tokio::test]
async fn submit_pre_compact_reflection_reports_history_failure_and_releases_slot() {
    let adapter = production_adapter();
    let binding = pre_compact_test_binding();
    let memory_config = share::config::MemoryConfig::default();
    let memory: Arc<dyn memory::MemoryPort> = Arc::new(memory::NoOpMemory);

    let outcome = submit_pre_compact_reflection(
        &adapter,
        &memory_config,
        &[Message::user("must not invoke provider")],
        &binding,
        "system prompt text",
        "en",
        &memory,
        &failing_append_reflection_history(),
    );

    assert_eq!(outcome, ReflectionTaskSubmitOutcome::Accepted);
    let completions = adapter.drain().await;
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].trigger, ReflectionTaskTrigger::PreCompact);
    assert_eq!(
        completions[0].status,
        crate::application::reflection::ReflectionTaskCompletionStatus::Failed
    );
    assert_eq!(
        completions[0]
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.error_category),
        Some(memory::api::ReflectionErrorCategory::History)
    );
    assert_eq!(
        adapter.submit(ReflectionTaskRequest::new(
            ReflectionTaskTrigger::PreCompact,
            vec![]
        )),
        ReflectionTaskSubmitOutcome::Accepted,
        "history append failure must release the shared slot"
    );
    adapter.cancel().await;
    let _ = adapter.drain().await;
}

/// Integration: the production compaction service and chat observer submit a PreCompact job exactly once
/// on `CompactOutcome::Committed`, using the early window the compact will
/// discard (not the empty recent tail).
#[tokio::test]
async fn pre_compact_trigger_submits_after_compact_outcome_committed() {
    let pre_compact_messages: Vec<Message> = (0..10)
        .map(|idx| Message::user(format!("u-{idx}")))
        .collect();
    let port_messages = pre_compact_messages.clone();
    let window = window_with(pre_compact_messages);
    let request = frozen_request();

    let harness = CompactHarness::new(Ok(CompactOutcome::Committed(CompactResult {
        summary: "summary".to_string(),
        recent_messages: vec![],
        source_revision: SessionRevision::new(7),
        quality: context::domain::CompactSummaryQuality::LocalOnly,
    })));

    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.initialize_for_launch(port_messages, 1);
    execution.replace_context_state(request, Some(window));
    let mut port = build_compact_test_port(&harness);

    let cancel = CancellationToken::new();
    let noop_progress = std::sync::Arc::new(|_: sdk::CompactStageView, _: sdk::CompactWorkView| {});
    let result = port.compact(&mut execution, &cancel, noop_progress).await;
    assert!(
        result.is_ok(),
        "compact should succeed on Committed: {result:?}"
    );

    // Drain the adapter to join the spawned PreCompact job; the completion
    // carries the trigger regardless of execution status (Succeeded / Failed).
    let completions = harness.adapter.drain().await;
    assert_eq!(
        completions.len(),
        1,
        "Committed must enqueue exactly one PreCompact job"
    );
    assert_eq!(
        completions[0].trigger,
        ReflectionTaskTrigger::PreCompact,
        "production PreCompact trigger must be submitted after Committed"
    );
    assert_eq!(harness.stub.compact_calls().len(), 1);
}

/// Integration: the production compaction service treats a Context-owned skip as a
/// non-fatal no-op and does not submit a PreCompact reflection.
#[tokio::test]
async fn pre_compact_trigger_skips_on_compact_outcome_skipped() {
    let window = window_with(vec![Message::user("only")]);
    let request = frozen_request();

    let harness = CompactHarness::new(Ok(CompactOutcome::Skipped(
        CompactSkipReason::ResumeProtection,
    )));

    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.initialize_for_launch(vec![Message::user("only")], 1);
    execution.replace_context_state(request, Some(window));
    let mut port = build_compact_test_port(&harness);

    let cancel = CancellationToken::new();
    let noop_progress = std::sync::Arc::new(|_: sdk::CompactStageView, _: sdk::CompactWorkView| {});
    let result = port.compact(&mut execution, &cancel, noop_progress).await;
    assert!(
        result.is_ok(),
        "automatic compact skip must continue the current Run: {result:?}"
    );

    // Allow any spawned tasks to settle so the absence of a submission is a
    // deterministic observation, not a race.
    let completions = harness.adapter.drain().await;
    assert!(
        completions.is_empty(),
        "Skipped must NOT submit a PreCompact reflection job: {completions:?}"
    );
    assert_eq!(harness.stub.compact_calls().len(), 1);
}

/// Integration: the production compaction service does NOT submit when the context port
/// returns an error from `compact`. The pre-compact snapshot must never be
/// observed by the reflection job because compact did not commit.
#[tokio::test]
async fn pre_compact_trigger_skips_when_context_compact_call_errors() {
    let window = window_with(vec![Message::user("only")]);
    let request = frozen_request();

    let harness = CompactHarness::new(Err(ContextPortError::Compact(
        "context port error".to_string(),
    )));

    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.initialize_for_launch(vec![Message::user("only")], 1);
    execution.replace_context_state(request, Some(window));
    let mut port = build_compact_test_port(&harness);

    let cancel = CancellationToken::new();
    let noop_progress = std::sync::Arc::new(|_: sdk::CompactStageView, _: sdk::CompactWorkView| {});
    let result = port.compact(&mut execution, &cancel, noop_progress).await;
    assert!(
        result.is_err(),
        "compact must propagate context port errors"
    );

    // Allow any spawned tasks to settle so the absence of a submission is a
    // deterministic observation, not a race.
    let completions = harness.adapter.drain().await;
    assert!(
        completions.is_empty(),
        "context port errors must NOT submit a PreCompact reflection job: {completions:?}"
    );
    assert_eq!(harness.stub.compact_calls().len(), 1);
}

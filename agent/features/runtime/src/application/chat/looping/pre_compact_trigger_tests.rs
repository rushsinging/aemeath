//! External tests for the production PreCompact reflection trigger (#1284).
//!
//! These tests verify that the production automatic compact path
//! (`MainRunPort::compact`) submits a `ReflectionTaskTrigger::PreCompact` job
//! using the **pre-compact** messages snapshot only when the context port
//! returns `CompactOutcome::Committed`. Errors and `CompactOutcome::Skipped`
//! must never enqueue a job. The submission shares the session-scoped
//! `ReflectionTaskAdapter` slot with `Interval` and `Manual` triggers; the
//! single-slot contention contract itself is already covered by the
//! `task_adapter_tests` in the reflection runner.

#![allow(clippy::type_complexity)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use sdk::{ChatId, ChatTurnId, RunId, RunStepId};
use share::config::domain::snapshot::ConfigSnapshot;
use share::config::Config;
use share::message::Message;
use tokio_util::sync::CancellationToken;

use super::loop_runner::main_run_port::{MainRunPort, StepMessageOwnership};
use super::task_reminder::TaskReminderState;
use crate::application::chat::looping::reflection::{
    maybe_submit_pre_compact_reflection, submit_pre_compact_reflection,
};
use crate::application::chat::looping::{
    ChatEventSink, EmptyInputEventDrainPort, EmptyQueueDrainPort, EventFuture, PendingInputBuffer,
    RuntimeStreamEvent, RuntimeTurnContext,
};
use crate::application::context_coordination::ContextCoordinator;
use crate::application::loop_engine::RunLoopPort;
use crate::application::reflection::{
    ReflectionTaskAdapter, ReflectionTaskRequest, ReflectionTaskSubmitOutcome,
    ReflectionTaskTrigger,
};
use crate::ports::{
    CalendarDate, CompactOutcome, CompactRequest, CompactResult, CompactSkipReason,
    CompactionDecision, ContextPort, ContextPortError, ContextRequest, ContextRequestId,
    ContextWindow, DecisionReason, Language as ContextLanguage, SessionId, SessionRevision,
    SystemBlock, SystemPromptSpec, TaskReminderSnapshot, TokenBudget, Urgency,
};

/// `submit_complete` builds its own executor closure and ignores the
/// adapter's `executor` field, so we cannot use a capturing closure to
/// observe submissions. The unit tests below therefore exercise the
/// helpers via the production adapter and a real provider whose response
/// parses as a (empty) reflection output. The integration tests against
/// `MainRunPort::compact` observe behavior through `adapter.drain()`,
/// which joins the spawned task and yields a `ReflectionTaskCompletion`
/// carrying the trigger regardless of execution status.
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
        system_prompt: SystemPromptSpec::new("system"),
        model_id: "fake/model".to_string(),
        effective_reasoning: provider::ReasoningLevel::Off,
        current_date: CalendarDate::new("2026-07-19"),
        task_reminder: TaskReminderSnapshot::default(),
        language: ContextLanguage::new("en"),
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

fn window_with(messages: Vec<Message>) -> ContextWindow {
    ContextWindow {
        backing_revision: SessionRevision::new(7),
        system_blocks: vec![SystemBlock {
            kind: "system_prompt".to_string(),
            content: "system".to_string(),
            cacheable: true,
        }],
        messages,
        tool_schemas: vec![],
        token_estimation: TokenBudget::default(),
        compaction_decision: CompactionDecision {
            needed: true,
            urgency: Urgency::Must,
            estimated_tokens: 0,
            threshold: 0,
            reason: DecisionReason::Heuristic,
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

/// Recording sink — required because `MainRunPort` is generic over the sink.
#[derive(Clone, Default)]
struct NullSink;

impl ChatEventSink for NullSink {
    fn send_event<'a>(&'a self, _event: RuntimeStreamEvent) -> EventFuture<'a> {
        Box::pin(async {})
    }

    fn try_send_event(&self, _event: RuntimeStreamEvent) {}
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

/// Inline builder for `MainRunPort`. Returns the port together with a
/// `Keepalive` struct that pins every `Arc`/owned value the port borrows so
/// the returned references stay valid for the test scope.
#[allow(clippy::too_many_lines)]
fn build_compact_test_port<'a>(
    harness: &'a mut CompactHarness,
    request: ContextRequest,
    window: Option<ContextWindow>,
    pre_compact_messages: Vec<Message>,
) -> MainRunPort<'a, NullSink, EmptyQueueDrainPort, EmptyInputEventDrainPort> {
    MainRunPort {
        sink: &harness.sink,
        queue: &harness.queue,
        input_events: &harness.input_events,
        binding: &harness.binding,
        tool_catalog: &harness.tool_catalog,
        tool_execution: &harness.tool_execution,
        tool_context_binding: &harness.tool_context_binding,
        system_prompt_text: "system",
        config_snapshot: &harness.config_snapshot,
        context: &harness.coordinator,
        context_request: Some(request),
        context_window: window,
        step_messages: StepMessageOwnership::new(pre_compact_messages.clone()),
        messages: pre_compact_messages,
        context_size: 128_000,
        workspace: &harness.workspace,
        session_id: "pre-compact-test",
        read_files: &harness.read_files,
        session_reminders: &harness.session_reminders,
        agent_runner: &None,
        tool_result_materializer: harness.tool_result_materializer.as_ref(),
        policy: &policy::AllowAllPolicy,
        task_access: &harness.task_access,
        max_tool_concurrency: 1,
        agent_semaphore: &harness.agent_semaphore,
        hook_runner: &harness.hook_runner,
        memory_config: &harness.memory_config,
        memory: &harness.memory,
        reflection_history: &harness.reflection_history,
        reflection_tasks: &harness.adapter,
        language: "en",
        reasoning: harness.reasoning.as_ref(),
        pending_input: &mut harness.pending_input,
        run_input_buffer: super::run_input_buffer::RunInputBuffer::new(),
        stop_hook_feedback: None,
        pending_stop_hook_feedback: None,
        pending_tool_results: false,
        per_turn_adopted: Vec::new(),
        cancel: CancellationToken::new(),
        run_id: RunId::new("run"),
        active_run: harness.active_run.as_ref(),
        turn_count: 1,
        turn_context: RuntimeTurnContext::new(ChatId::new_v7(), ChatTurnId::new_v7()),
        last_total_tokens: &mut harness.last_total_tokens,
        task_reminder_state: &mut harness.task_reminder_state,
        tool_identity: &harness.tool_identity,
        started_at: Instant::now(),
    }
}

use workflow::api::{ReasoningNode, ReasoningObservation, ReasoningPort, ReasoningSignal};

struct StubReasoningPort;

impl ReasoningPort for StubReasoningPort {
    fn observe(&self, _signal: ReasoningSignal) -> ReasoningObservation {
        ReasoningObservation {
            previous: ReasoningNode::Idle,
            current: ReasoningNode::Idle,
            requested: self.current_requested_level(),
        }
    }

    fn current_requested_level(&self) -> share::reasoning::ReasoningLevel {
        share::reasoning::ReasoningLevel::Off
    }

    fn set_level(
        &self,
        level: share::reasoning::ReasoningLevel,
    ) -> share::reasoning::ReasoningLevel {
        level
    }

    fn reset_default_level(
        &self,
        level: share::reasoning::ReasoningLevel,
    ) -> share::reasoning::ReasoningLevel {
        level
    }
}

/// Per-test harness. Holds all owned state that the borrowed `MainRunPort`
/// references. Must outlive the port that `build_compact_test_port` returns.
struct CompactHarness {
    adapter: ReflectionTaskAdapter,
    coordinator: ContextCoordinator,
    stub: Arc<StubContextPort>,
    binding: Arc<crate::ports::ProviderBinding>,
    memory: Arc<dyn memory::MemoryPort>,
    reflection_history: Arc<dyn memory::api::ReflectionHistoryStore>,
    memory_config: share::config::MemoryConfig,
    tool_result_materializer:
        Arc<crate::application::tool_result_materialization::ToolResultMaterializer>,
    config_snapshot: ConfigSnapshot,
    hook_runner: Arc<dyn hook::HookPort>,
    workspace: project::WorkspaceViews,
    tool_catalog: Arc<dyn tools::ToolCatalogPort>,
    tool_execution: Arc<dyn tools::ToolExecutionPort>,
    tool_context_binding: Arc<dyn tools::ToolExecutionContextBindingPort>,
    read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    session_reminders: Arc<std::sync::Mutex<::tools::SessionReminders>>,
    active_run: Arc<crate::application::active_run::ActiveRunRegistry>,
    task_access: Arc<dyn task::TaskAccess>,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    sink: NullSink,
    queue: EmptyQueueDrainPort,
    input_events: EmptyInputEventDrainPort,
    pending_input: PendingInputBuffer,
    last_total_tokens: Option<u64>,
    task_reminder_state: TaskReminderState,
    tool_identity: crate::application::tool_coordination::identity::ToolIdentityRegistry,
    reasoning: Arc<dyn workflow::api::ReasoningPort>,
}

impl CompactHarness {
    fn new(outcome: Result<CompactOutcome, ContextPortError>) -> Self {
        let adapter = production_adapter();
        let stub = StubContextPort::new(outcome);
        let coordinator = ContextCoordinator::new(stub.clone());
        let binding = pre_compact_test_binding();
        let memory: Arc<dyn memory::MemoryPort> = Arc::new(memory::NoOpMemory);
        let reflection_history = noop_reflection_history();
        let memory_config = share::config::MemoryConfig::default();
        let tool_result_materializer = crate::application::testing::test_tool_result_materializer();
        let config_snapshot = ConfigSnapshot::new(Config::default());
        let hook_events = HashMap::new();
        let hook_runner: Arc<dyn hook::HookPort> = Arc::new(
            hook::build_dispatcher(
                &share::config::hooks::HooksConfig {
                    events: hook_events,
                },
                std::collections::HashMap::new(),
            )
            .unwrap(),
        );
        let workspace = project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views();
        let tool_catalog =
            ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port();
        let tool_execution = ::tools::composition::TestCatalogExecutionFactory::empty().execution();
        let tool_context_binding =
            ::tools::composition::TestCatalogExecutionFactory::empty().binding();
        let read_files = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let session_reminders = Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new()));
        let active_run = Arc::new(crate::application::active_run::ActiveRunRegistry::default());
        let task_access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
        let agent_semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let reasoning: Arc<dyn workflow::api::ReasoningPort> = Arc::new(StubReasoningPort);
        Self {
            adapter,
            coordinator,
            stub,
            binding,
            memory,
            reflection_history,
            memory_config,
            tool_result_materializer,
            config_snapshot,
            hook_runner,
            workspace,
            tool_catalog,
            tool_execution,
            tool_context_binding,
            read_files,
            session_reminders,
            active_run,
            task_access,
            agent_semaphore,
            sink: NullSink,
            queue: EmptyQueueDrainPort,
            input_events: EmptyInputEventDrainPort,
            pending_input: PendingInputBuffer::default(),
            last_total_tokens: Some(0),
            task_reminder_state: TaskReminderState::new(),
            tool_identity:
                crate::application::tool_coordination::identity::ToolIdentityRegistry::new(),
            reasoning,
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
        Ok(crate::application::testing::text_completion_stream(
            r#"{"deviations":[],"suggested_memories":[],"outdated_memories":[]}"#,
            1,
            1,
        ))
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

/// Integration: `MainRunPort::compact` submits a PreCompact job exactly once
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

    let mut harness = CompactHarness::new(Ok(CompactOutcome::Committed(CompactResult {
        summary: "summary".to_string(),
        recent_messages: vec![],
        source_revision: SessionRevision::new(7),
    })));

    let mut port = build_compact_test_port(&mut harness, request, Some(window), port_messages);

    let cancel = CancellationToken::new();
    let result = port.compact(&cancel).await;
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

/// Integration: `MainRunPort::compact` does NOT submit when the context port
/// returns `CompactOutcome::Skipped`, even though `compact()` itself surfaces
/// the skip as an adapter error.
#[tokio::test]
async fn pre_compact_trigger_skips_on_compact_outcome_skipped() {
    let window = window_with(vec![Message::user("only")]);
    let request = frozen_request();

    let mut harness = CompactHarness::new(Ok(CompactOutcome::Skipped(
        CompactSkipReason::ResumeProtection,
    )));

    let mut port = build_compact_test_port(
        &mut harness,
        request,
        Some(window),
        vec![Message::user("only")],
    );

    let cancel = CancellationToken::new();
    let result = port.compact(&cancel).await;
    assert!(result.is_err(), "compact must surface Skipped as an error");

    // Allow any spawned tasks to settle so the absence of a submission is a
    // deterministic observation, not a race.
    let completions = harness.adapter.drain().await;
    assert!(
        completions.is_empty(),
        "Skipped must NOT submit a PreCompact reflection job: {completions:?}"
    );
    assert_eq!(harness.stub.compact_calls().len(), 1);
}

/// Integration: `MainRunPort::compact` does NOT submit when the context port
/// returns an error from `compact`. The pre-compact snapshot must never be
/// observed by the reflection job because compact did not commit.
#[tokio::test]
async fn pre_compact_trigger_skips_when_context_compact_call_errors() {
    let window = window_with(vec![Message::user("only")]);
    let request = frozen_request();

    let mut harness = CompactHarness::new(Err(ContextPortError::Compact(
        "context port error".to_string(),
    )));

    let mut port = build_compact_test_port(
        &mut harness,
        request,
        Some(window),
        vec![Message::user("only")],
    );

    let cancel = CancellationToken::new();
    let result = port.compact(&cancel).await;
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

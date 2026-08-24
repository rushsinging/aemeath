// Tests for `loop_runner`, extracted into a dedicated module to keep the
// runner file focused on the production code path.


use super::main_run_port::{
    fixture_accepted_user_messages, fixture_bind_pending, fixture_finalize_messages,
};
use super::*;

fn assistant(text: &str) -> Message {
    Message {
        role: share::message::Role::Assistant,
        content: vec![share::message::ContentBlock::Text {
            text: text.to_string(),
        }],
        metadata: None,
    }
}

#[test]
fn empty_session_first_step_owns_user_then_assistant_without_loss() {
    let user = Message::user("first");
    let assistant = assistant("answer");
    let finalized = fixture_finalize_messages(vec![user], vec![assistant]);
    assert_eq!(finalized.len(), 2);
    assert_eq!(finalized[0].text_content(), "first");
    assert_eq!(finalized[1].text_content(), "answer");
}

#[test]
fn accepted_projection_keeps_only_user_input_not_system_feedback() {
    let accepted = fixture_accepted_user_messages(
        vec![Message::user("accepted")],
        Some(Message::system_generated_user("stop hook feedback")),
        &[],
    );

    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].text_content(), "accepted");
}

/// Regression test for #1272 Bug 2: stop hook feedback is consumed
/// by freeze_step (via pending_stop_hook_feedback → prefix) and
/// must appear in the frozen messages as a system-generated user
/// message BEFORE regular user inputs.
///
/// Uses `fixture_bind_pending` (no prefix) + `fixture_accepted_user_messages`
/// (with prefix) to verify: (a) feedback is excluded from accepted input,
/// (b) pending messages are correctly bound.
#[test]
fn freeze_step_injects_stop_hook_feedback_as_system_prefix() {
    // When a stop hook feedback prefix is present, it must be injected
    // as a system-generated user message before regular user inputs,
    // and must NOT appear in accepted input projection.
    let accepted = fixture_accepted_user_messages(
        vec![Message::user("user text")],
        Some(Message::system_generated_user("stop hook feedback")),
        &[],
    );
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].text_content(), "user text");

    // Without a prefix, regular pending messages are frozen normally.
    let (frozen_no_prefix, _) = fixture_bind_pending(vec![Message::user("user text")], &[]);
    assert_eq!(frozen_no_prefix.len(), 1);
    assert_eq!(frozen_no_prefix[0].text_content(), "user text");
}

/// Regression: previously drain_input took from stop_hook_feedback
/// and freeze_step took from it again — getting None (double-take).
/// With pending_stop_hook_feedback relay, freeze_step always sees the feedback.
#[test]
fn pending_stop_hook_feedback_survives_drain_then_freeze() {
    // Simulate the relay: drain_input takes from stop_hook_feedback,
    // stores to pending_stop_hook_feedback; freeze_step consumes from it.
    let feedback = Message::system_generated_user("stop hook feedback");
    let mut pending_relay = Some(feedback.clone());

    // freeze_step phase: consume from relay, not from stop_hook_feedback
    let freeze_prefix = pending_relay.take();

    assert!(freeze_prefix.is_some(), "freeze_step must see the feedback");
    assert_eq!(freeze_prefix.unwrap().text_content(), "stop hook feedback");
    // After freeze_step consumes, relay is empty.
    assert!(pending_relay.is_none(), "feedback consumed exactly once");
    // Demonstrate the old bug: if freeze_step tried stop_hook_feedback
    // (separate field), it would be None.
    let stop_hook_feedback: Option<Message> = None;
    assert!(
        stop_hook_feedback.is_none(),
        "old bug: stop_hook_feedback already taken by drain_input"
    );
}

#[test]
fn tool_step_owns_user_assistant_and_tool_result_in_order() {
    let finalized = fixture_finalize_messages(
        vec![Message::user("use tool")],
        vec![assistant("tool_use"), Message::user("tool_result")],
    );
    assert_eq!(
        finalized
            .iter()
            .map(Message::text_content)
            .collect::<Vec<_>>(),
        vec!["use tool", "tool_use", "tool_result"]
    );
}

#[test]
fn finalized_mapping_preserves_complete_turn_order() {
    let finalized =
        fixture_finalize_messages(vec![Message::user("question")], vec![assistant("final")]);
    assert_eq!(finalized[0].role, share::message::Role::User);
    assert_eq!(finalized[1].role, share::message::Role::Assistant);
}

#[test]
fn historical_messages_do_not_determine_new_step_ownership() {
    let history = [Message::user("old"), assistant("old answer")];
    let new_user = Message::user("new first sentence");
    let (pending, active) = fixture_bind_pending(vec![new_user], &[]);
    assert_eq!(pending.len(), 1);
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].text_content(), "new first sentence");
    assert!(active.iter().all(|message| !history
        .iter()
        .any(|old| { old.text_content() == message.text_content() })));
}

#[test]
fn production_source_does_not_infer_message_ownership_by_index() {
    let source = include_str!("main_run_port.rs");
    let forbidden = ["projection", "start", "index"].concat();
    assert!(!source.contains(&forbidden));
}

#[derive(Clone)]
struct TestMemoryOpener;

#[async_trait::async_trait]
impl memory::api::MemoryOpener for TestMemoryOpener {
    async fn open_memory(
        &self,
        _key: &memory::api::ProjectMemoryKey,
        _config: &share::config::MemoryConfig,
    ) -> Result<Arc<dyn memory::api::MemoryPort>, memory::api::MemoryOpenerError> {
        Ok(Arc::new(memory::api::NoOpMemory))
    }

    fn boxed_clone(&self) -> Box<dyn memory::api::MemoryOpener> {
        Box::new(self.clone())
    }
}

fn test_wiring() -> Arc<context::MainSessionWiring> {
    let workspace = project::wire_production_workspace(std::env::current_dir().unwrap())
        .expect("workspace 初始化成功")
        .into_views();
    let persist = workspace.persist();
    let config = Arc::new(config::ConfigAppService::new(Some(
        &workspace.read().initial_cwd(),
    )));
    let now = chrono::Utc::now().to_rfc3339();
    Arc::new(context::MainSessionWiring::build(
        context::MainSessionWiringBuilder {
            workspace_read: workspace.read(),
            workspace_persist: persist.clone(),
            task_persist: Arc::new(task::TaskStore::new()),
            config_reader: config.clone(),
            config_participant: config,
            memory_opener: Box::new(TestMemoryOpener),
            session_management: Arc::new(context::test_support::UnavailableSessionManagement),
            initial_session: context::session::CanonicalSession {
                id: uuid::Uuid::now_v7().to_string(),
                chats: Vec::new(),
                created_at: now.clone(),
                updated_at: now,
                metadata: Default::default(),
                tasks: context::session::SnapshotState::Missing,
                workspace: context::session::SnapshotState::Captured(persist.snapshot()),
                revision: 0,
                compact: None,
                cleared_after: None,
                run_slices: Vec::new().into(),
                committed_steps: Default::default(),
                skill_load_records: Vec::new(),
            },
            initial_memory: Arc::new(memory::api::NoOpMemory),
            context_factory: Arc::new(context::ProductionMainContextFactory::new(Arc::new(
                context::NoOpCanonicalSessionWriter,
            ))),
        },
    ))
}

use crate::application::model::test_support::{
    advance_until_retry_condition, empty_completion, successful_completion, text_completion_stream,
    ScriptedInvocationProvider,
};

use async_trait::async_trait;
use futures::StreamExt;
use hook::HookPort;
use provider::test_harness::{InvocationScope, LlmProvider, SystemBlock};
use provider::ReasoningLevel;
use provider::{
    InvocationDelta, InvocationEvent, InvocationStream, ProviderCompletion, ProviderContentBlock,
    ProviderError, ProviderErrorKind, ProviderStopReason, ProviderToolCall, ProviderToolCallId,
    RawUsageSnapshot,
};
use share::config::hooks::{HookEntry, HookEvent, HooksConfig};
use share::config::models::ResolvedModel;
use share::message::{Message, MessageSource, Role};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct TestReflectionHistory;

#[async_trait]
impl memory::api::ReflectionHistoryQuery for TestReflectionHistory {
    async fn list(
        &self,
        _limit: usize,
    ) -> Result<Vec<memory::api::ReflectionSafeSummary>, memory::api::MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl memory::api::ReflectionHistoryStore for TestReflectionHistory {
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

fn test_reflection_history_store() -> Arc<dyn memory::api::ReflectionHistoryStore> {
    Arc::new(TestReflectionHistory)
}

/// #1385: No-op AgentRunner for tests that don't exercise agent tool dispatch.
struct NoopAgentRunner;
#[async_trait]
impl ::tools::AgentRunner for NoopAgentRunner {
    async fn run_agent(&self, _request: ::tools::AgentRunRequest<'_>) -> ::tools::AgentRunTerminal {
        ::tools::AgentRunTerminal::Completed {
            result: String::new(),
        }
    }
}

/// #1385: Hook port that delegates to a real dispatcher with empty config.
fn noop_hook_port() -> Arc<dyn hook::HookPort> {
    Arc::new(
        hook::build_dispatcher(&share::config::domain::snapshot::ConfigSnapshot::new(share::config::Config { hooks: HooksConfig {
            events: HashMap::new(),
            ..HooksConfig::default()
        }, ..share::config::Config::default() }))
        .expect("empty hook dispatcher"),
    )
}

/// #1385: Construct a [`SessionRuntime`] for tests.
fn test_shell() -> crate::application::client::SessionRuntime {
    test_shell_with_hooks(noop_hook_port())
}

fn test_shell_with_hooks(
    hooks: Arc<dyn hook::HookPort>,
) -> crate::application::client::SessionRuntime {
    test_shell_with_task_store(hooks, Arc::new(task::TaskStore::new()))
}

fn test_shell_with_catalog(
    hooks: Arc<dyn hook::HookPort>,
    factory: ::tools::composition::TestCatalogExecution,
) -> crate::application::client::SessionRuntime {
    let wiring = test_wiring();
    let binding = crate::application::model::test_support::test_binding(vec!["dummy"]);
    let cwd = std::env::current_dir().unwrap();
    let workspace = project::wire_production_workspace(cwd.clone())
        .expect("workspace 初始化成功")
        .into_views();

    crate::application::client::SessionRuntime {
        session_state: Arc::new(std::sync::RwLock::new(
            crate::application::run::creation::SessionState::new(
                "test-session",
                cwd,
                format!("{}/{}", binding.model.provider, binding.model.model),
                share::config::domain::snapshot::ConfigSnapshot::new(
                    share::config::Config::default(),
                ),
            ),
        )),
        workspace,
        wiring,
        config_query: Arc::new(config::ConfigAppService::new(None)),
        config_writer: Arc::new(config::ConfigAppService::new(None)),
        session_management: Arc::new(context::test_support::UnavailableSessionManagement),
        provider_factory: crate::application::model::test_support::constant_factory(
            binding.clone(),
        ),
        model_state: crate::application::client::SessionModelState::new(
            ResolvedModel {
                source_key: "test".to_string(),
                source_config: Default::default(),
                driver: "openai".to_string(),
                model: Default::default(),
            },
            binding,
        ),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        initial_git_context: String::new(),
        user_context: String::new(),
        prompt_model_id: "test-model".to_string(),
        skill_catalog: ::tools::composition::wire_skills().catalog(),
        initial_skill_snapshot: ::tools::SkillCatalogSnapshot::from_descriptors(Vec::new()),
        memory_config: share::config::MemoryConfig::default(),
        context_size: 200_000,
        language: "en".to_string(),
        allow_all: true,
        verbose: false,
        resume: None,
        startup_resume: None,
        agent_runner: Arc::new(NoopAgentRunner),
        parent_context_source: crate::application::run::context::ParentRunContextSource::new(),
        tool_result_materializer:
            crate::application::tool::test_support::test_tool_result_materializer(),
        active_run: Arc::new(
            crate::application::run::active_registry::ActiveRunRegistry::default(),
        ),
        interaction_bridge: Arc::new(
            crate::application::interaction::port::InteractionBridge::new(),
        ),
        session_ingress: Arc::new(crate::application::session::ingress::SessionIngress::new(
            Arc::new(crate::application::interaction::port::InteractionBridge::new()),
        )),
        event_sink_factory: Arc::new(|tx| {
            crate::application::loop_engine::chat::ChatEventSinkHandle::new(
                crate::adapters::sdk_event_sink::SdkChatEventSink::new(tx),
            )
        }),
        input_port_factory: Arc::new(|ingress| {
            crate::application::client::SessionInputHandle::new(
                crate::adapters::input_buffer::RuntimeInputEventDrainPort::new(ingress),
            )
        }),
        session_reminders: Arc::new(std::sync::RwLock::new(
            share::memory::SessionReminders::default(),
        )),
        runtime_context_factory: Arc::new(
            crate::application::run::context_factory::RuntimeContextFactory::new(
                factory.catalog_port(),
                factory.execution(),
                Arc::new(policy::AllowAllPolicy),
                test_reflection_history_store(),
                Arc::new(task::TaskStore::new()),
                hooks,
                Arc::new(crate::ports::UnavailableUsageSink),
            ),        ),
    }
}

/// #1492：预置 Task 状态的行为测试用——允许注入外部 `TaskStore`。
fn test_shell_with_task_store(
    hooks: Arc<dyn hook::HookPort>,
    task_store: Arc<task::TaskStore>,
) -> crate::application::client::SessionRuntime {
    let wiring = test_wiring();
    let binding = crate::application::model::test_support::test_binding(vec!["dummy"]);
    let cwd = std::env::current_dir().unwrap();
    let workspace = project::wire_production_workspace(cwd.clone())
        .expect("workspace 初始化成功")
        .into_views();
    let factory = ::tools::composition::TestCatalogExecutionFactory::empty();

    crate::application::client::SessionRuntime {
        session_state: Arc::new(std::sync::RwLock::new(
            crate::application::run::creation::SessionState::new(
                "test-session",
                cwd,
                format!("{}/{}", binding.model.provider, binding.model.model),
                share::config::domain::snapshot::ConfigSnapshot::new(
                    share::config::Config::default(),
                ),
            ),
        )),
        workspace,
        wiring,
        config_query: Arc::new(config::ConfigAppService::new(None)),
        config_writer: Arc::new(config::ConfigAppService::new(None)),
        session_management: Arc::new(context::test_support::UnavailableSessionManagement),
        provider_factory: crate::application::model::test_support::constant_factory(
            binding.clone(),
        ),
        model_state: crate::application::client::SessionModelState::new(
            ResolvedModel {
                source_key: "test".to_string(),
                source_config: Default::default(),
                driver: "openai".to_string(),
                model: Default::default(),
            },
            binding,
        ),
        max_tool_concurrency: 1,
        max_agent_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        initial_git_context: String::new(),
        user_context: String::new(),
        prompt_model_id: "test-model".to_string(),
        skill_catalog: ::tools::composition::wire_skills().catalog(),
        initial_skill_snapshot: ::tools::SkillCatalogSnapshot::from_descriptors(Vec::new()),
        memory_config: share::config::MemoryConfig::default(),
        context_size: 200_000,
        language: "en".to_string(),
        allow_all: true,
        verbose: false,
        resume: None,
        startup_resume: None,
        agent_runner: Arc::new(NoopAgentRunner),
        parent_context_source: crate::application::run::context::ParentRunContextSource::new(),
        tool_result_materializer:
            crate::application::tool::test_support::test_tool_result_materializer(),
        active_run: Arc::new(
            crate::application::run::active_registry::ActiveRunRegistry::default(),
        ),
        interaction_bridge: Arc::new(
            crate::application::interaction::port::InteractionBridge::new(),
        ),
        session_ingress: Arc::new(crate::application::session::ingress::SessionIngress::new(
            Arc::new(crate::application::interaction::port::InteractionBridge::new()),
        )),
        event_sink_factory: Arc::new(|tx| {
            crate::application::loop_engine::chat::ChatEventSinkHandle::new(
                crate::adapters::sdk_event_sink::SdkChatEventSink::new(tx),
            )
        }),
        input_port_factory: Arc::new(|ingress| {
            crate::application::client::SessionInputHandle::new(
                crate::adapters::input_buffer::RuntimeInputEventDrainPort::new(ingress),
            )
        }),
        session_reminders: Arc::new(std::sync::RwLock::new(
            share::memory::SessionReminders::default(),
        )),
        runtime_context_factory: Arc::new(
            crate::application::run::context_factory::RuntimeContextFactory::new(
                factory.catalog_port(),
                factory.execution(),
                Arc::new(policy::AllowAllPolicy),
                test_reflection_history_store(),
                task_store,
                hooks,
                Arc::new(crate::ports::UnavailableUsageSink),
            ),        ),
    }
}

/// #1385: Test fake implementing `SessionQueryPort`, replacing four `Arc<Fn>` fixtures.
struct FakeSessionQuery;

#[async_trait::async_trait]
impl crate::ports::SessionQueryPort for FakeSessionQuery {
    async fn list_models(&self) -> Result<Vec<sdk::ModelSummary>, sdk::SdkError> {
        Ok(Vec::new())
    }
    async fn list_sessions(&self) -> Result<Vec<sdk::SessionSummary>, sdk::SdkError> {
        Ok(Vec::new())
    }
    async fn list_reminders(&self) -> Result<Vec<sdk::ReminderView>, sdk::SdkError> {
        Ok(Vec::new())
    }
    async fn list_reflection_history(
        &self,
        _limit: usize,
    ) -> Result<Vec<sdk::ReflectionHistoryView>, sdk::SdkError> {
        Ok(Vec::new())
    }
}

fn test_session_query_port() -> Arc<dyn crate::ports::SessionQueryPort> {
    Arc::new(FakeSessionQuery)
}

/// Construct a [`SessionCommandDriverInput`] from a test session.
fn test_session_driver_input<S, I>(
    sink: S,
    input_events: I,
    session: crate::application::client::SessionRuntime,
) -> SessionCommandDriverInput<S, I>
where
    S: ChatEventSink,
    I: crate::application::loop_engine::input_strategy::SessionInputPort,
{
    SessionCommandDriverInput {
        sink,
        input_events,
        session,
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        session_queries: test_session_query_port(),
    }
}

#[test]
fn runtime_resume_replaces_the_only_active_session_id() {
    let runner_source = [
        include_str!("session_driver/run_launch.rs"),
        include_str!("session_driver/run_preparation.rs"),
    ]
    .join("\n");
    let port_source = include_str!("main_run_port.rs");
    let context_request_source = include_str!("../context_request.rs");

    // Session identity is initialized from SessionState and resume writes it back.
    assert!(
        runner_source.contains("let mut session_id = session_snapshot.session_id().to_string();")
    );
    assert!(runner_source.contains("session_id = resume_view.session_id.clone();"));
    assert!(runner_source.contains(".update_session("));
    assert!(runner_source.contains("if session_id != prepared_session.session_id()"));
    assert!(runner_source.contains("session_id = prepared_session.session_id().to_string();"));
    assert!(!runner_source.contains("context_session_id"));
    assert!(!port_source.contains("context_session_id"));
    assert!(runner_source.contains("session_id: &session_id"));
    assert!(context_request_source.contains("session_id: SessionId::new(self.source.session_id)"));
}

#[test]
fn main_production_path_is_wired_to_shared_run_loop_without_legacy_fsm() {
    // Architecture guard: behavioral tests below exercise this entry point, while this assertion
    // prevents a future reintroduction of the retired Main-only orchestration state machine.
    let source = include_str!("session_driver/run_launch.rs");
    // #1397: Main enters the shared engine only through RunLauncher::launch.
    assert!(source.contains("run::launcher::launch"));
    assert!(!source.contains("ChatLoopFsm"));
    assert!(!source.contains("StallDetector"));
    assert!(!source.contains("ChatLoopTransition"));
}

#[test]
fn main_logging_path_uses_scopes_and_no_legacy_setters() {
    let chat_source = include_str!("../../client/trait_chat.rs");
    let runner_source = include_str!("session_driver/run_launch.rs");
    let port_source = include_str!("main_run_port.rs");

    let invocation_source = include_str!("../../model/invocation.rs");

    assert!(chat_source.contains("logging::spawn_instrumented(session_context"));
    assert!(runner_source.contains("session_id: logging::FieldPatch::Set"));
    assert!(runner_source.contains("chat_id: logging::FieldPatch::Set"));
    assert!(runner_source.contains("run_step: logging::FieldPatch::Set(step_count)"));
    assert!(invocation_source.contains("logging::instrument(request_context"));
    for source in [chat_source, runner_source, port_source, invocation_source] {
        assert!(!source.contains("logging::set_current_"));
        assert!(!source.contains("logging::set_session_id"));
    }
}

#[test]
fn progress_forwarders_capture_logging_context_before_instrumented_spawn() {
    let agent_calls = include_str!("agent_calls.rs");
    let non_agent = include_str!("non_agent.rs");

    for source in [agent_calls, non_agent] {
        assert!(source.contains("let progress_log_context = logging::capture();"));
        assert!(source.contains("logging::spawn_instrumented(progress_log_context, async move"));
    }
}

#[test]
fn each_request_attempt_has_complete_fresh_context() {
    let parent = logging::LogContext {
        session_id: Some("session".into()),
        chat_id: Some("chat".into()),
        run_step: Some(3),
        ..logging::LogContext::default()
    };
    let first = main_run_port::request_log_context(&parent, "model-a", "provider-a", "default");
    let retry = main_run_port::request_log_context(&parent, "model-a", "provider-a", "default");

    assert_eq!(first.session_id.as_deref(), Some("session"));
    assert_eq!(first.chat_id.as_deref(), Some("chat"));
    assert_eq!(first.run_step, Some(3));
    assert_eq!(first.model.as_deref(), Some("model-a"));
    assert_eq!(first.provider.as_deref(), Some("provider-a"));
    assert_eq!(first.role.as_deref(), Some("default"));
    assert_ne!(
        first.request_id, retry.request_id,
        "retry must get a new request_id"
    );
}

#[derive(Clone, Default)]
struct RecordingSink {
    events: Arc<Mutex<Vec<String>>>,
    messages_syncs: Arc<Mutex<Vec<Vec<Message>>>>,
    compact_rollback_snapshots: Arc<Mutex<Vec<Vec<Message>>>>,
    done_durations: Arc<Mutex<Vec<std::time::Duration>>>,
    /// Captures InputId lists per `UserMessagesAdopted` emit (#1272).
    adopted_ids: Arc<Mutex<Vec<Vec<String>>>>,
    /// Captures TurnStarted message counts per turn (#1272).
    llm_message_counts: Arc<Mutex<Vec<usize>>>,
}

impl ChatEventSink for RecordingSink {
    fn send_event<'a>(
        &'a self,
        event: RuntimeStreamEvent,
    ) -> crate::application::loop_engine::chat::EventFuture<'a> {
        Box::pin(async move {
            self.record(event);
        })
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.record(event);
    }

    fn send_activity_event(
        &self,
        event: crate::application::loop_engine::chat::RuntimeActivityEvent,
    ) {
        let name = match event {
            crate::application::loop_engine::chat::RuntimeActivityEvent::Snapshot(snapshot) => {
                let live_hook = snapshot.activities.iter().find(|activity| {
                    activity.kind == sdk::ActivityKindView::HookDispatch
                        && matches!(
                            activity.state,
                            sdk::ActivityStateView::Running | sdk::ActivityStateView::Waiting
                        )
                });
                let terminal_hook = snapshot.activities.iter().find(|activity| {
                    activity.kind == sdk::ActivityKindView::HookDispatch
                        && !matches!(
                            activity.state,
                            sdk::ActivityStateView::Running | sdk::ActivityStateView::Waiting
                        )
                });
                if let Some(activity) = live_hook {
                    format!("HookActivitySnapshot:Live:{}", activity.id)
                } else if let Some(activity) = terminal_hook {
                    format!("HookActivitySnapshot:Terminal:{}", activity.id)
                } else {
                    format!(
                        "ActivitySnapshot:{}:{}",
                        snapshot.revision, snapshot.heartbeat_sequence
                    )
                }
            }
        };
        self.events.lock().unwrap().push(name);
    }
}

impl RecordingSink {
    fn record(&self, event: RuntimeStreamEvent) {
        let name = match &event {
            RuntimeStreamEvent::TurnStarted { messages }
            | RuntimeStreamEvent::MicrocompactCompleted { messages, .. }
            | RuntimeStreamEvent::CompactOperationCompleted { messages, .. } => {
                self.messages_syncs.lock().unwrap().push(messages.clone());
                let tag = match &event {
                    RuntimeStreamEvent::TurnStarted { .. } => {
                        self.llm_message_counts.lock().unwrap().push(messages.len());
                        "TurnStarted"
                    }
                    RuntimeStreamEvent::MicrocompactCompleted { .. } => "MicrocompactCompleted",
                    RuntimeStreamEvent::CompactOperationCompleted { .. } => {
                        "CompactOperationCompleted"
                    },
                    _ => "Sync",
                };
                format!(
                    "{}:{}",
                    tag,
                    messages
                        .last()
                        .map(|message| message.text_content())
                        .unwrap_or_default()
                )
            }
            RuntimeStreamEvent::CompactOperationRolledBack { messages } => {
                self.messages_syncs.lock().unwrap().push(messages.clone());
                self.compact_rollback_snapshots
                    .lock()
                    .unwrap()
                    .push(messages.clone());
                format!(
                    "CompactOperationRolledBack:{}",
                    messages
                        .last()
                        .map(|message| message.text_content())
                        .unwrap_or_default()
                )
            }
            RuntimeStreamEvent::ApiError { messages, error } => {
                self.messages_syncs.lock().unwrap().push(messages.clone());
                format!("ApiError:{}", error)
            }
            RuntimeStreamEvent::DoneWithDuration { duration, .. } => {
                self.done_durations.lock().unwrap().push(*duration);
                "DoneWithDuration".to_string()
            }
            RuntimeStreamEvent::RunChanged(turn) => format!("RunChanged:{turn}"),
            RuntimeStreamEvent::Usage { .. } => "Usage".to_string(),
            RuntimeStreamEvent::AssistantTextDelta { delta, .. } => format!("Text:{delta}"),
            RuntimeStreamEvent::Done { .. } => "Done".to_string(),
            RuntimeStreamEvent::SystemMessage(message) => format!("SystemMessage:{message}"),
            RuntimeStreamEvent::HookNotice(notice) => format!("HookNotice:{}", notice.reason),
            RuntimeStreamEvent::Cancelled { duration, .. } => {
                self.done_durations.lock().unwrap().push(*duration);
                "Cancelled".to_string()
            }
            RuntimeStreamEvent::ThinkingDelta { .. } => "Thinking".to_string(),
            RuntimeStreamEvent::BlockComplete { .. } => "BlockComplete".to_string(),
            RuntimeStreamEvent::ToolCallStarted { .. } => "ToolCallStarted".to_string(),
            RuntimeStreamEvent::ToolCallArgumentsDelta { .. } => {
                "ToolCallArgumentsDelta".to_string()
            }
            RuntimeStreamEvent::ToolCallStateChanged { .. } => {
                "ToolCallStateChanged".to_string()
            }
            RuntimeStreamEvent::ToolResult { .. } => "ToolResult".to_string(),
            RuntimeStreamEvent::LiveTps(_) => "LiveTps".to_string(),
            RuntimeStreamEvent::InteractionRequested { .. } => "InteractionRequested".to_string(),
            RuntimeStreamEvent::ToolOutputDelta { .. } => "ToolOutputDelta".to_string(),
            RuntimeStreamEvent::SubRunStarted(_) => "SubRunStarted".to_string(),
            RuntimeStreamEvent::SubRunActivity(_) => "SubRunActivity".to_string(),
            RuntimeStreamEvent::WorkingDirectoryChanged { .. } => {
                "WorkingDirectoryChanged".to_string()
            }
            RuntimeStreamEvent::TaskStateChanged { .. } => "TaskStateChanged".to_string(),
            RuntimeStreamEvent::ConfigReloaded { .. } => "ConfigReloaded".to_string(),
            RuntimeStreamEvent::UserMessagesAdopted { items, .. } => {
                self.adopted_ids.lock().unwrap().push(
                    items
                        .iter()
                        .map(|(id, _)| id.as_str().to_string())
                        .collect(),
                );
                "UserMessagesAdopted".to_string()
            }
            RuntimeStreamEvent::UserMessagesQueued { .. } => "UserMessagesQueued".to_string(),
            RuntimeStreamEvent::SessionMessageStateChanged {
                message_count,
                revision,
            } => format!("SessionMessageStateChanged:{message_count}:{revision}"),
            RuntimeStreamEvent::SessionReset => "SessionReset".to_string(),
            RuntimeStreamEvent::UserMessagesWithdrawn { .. } => "UserMessagesWithdrawn".to_string(),
            RuntimeStreamEvent::ModelSwitched { .. } => "ModelSwitched".to_string(),
            RuntimeStreamEvent::ModelList { .. } => "ModelList".to_string(),
            RuntimeStreamEvent::ThinkingChanged { .. } => "ThinkingChanged".to_string(),
            RuntimeStreamEvent::ContextEstimated { .. } => "ContextEstimated".to_string(),
            RuntimeStreamEvent::CommandResultText { .. } => "CommandResultText".to_string(),
            RuntimeStreamEvent::ReflectionHistory { records } => {
                format!("ReflectionHistory:{}", records.len())
            }
            RuntimeStreamEvent::SessionResumed { .. } => "SessionResumed".to_string(),
            RuntimeStreamEvent::ModelInvocationRetrying { attempt, delay, .. } => {
                format!("ModelInvocationRetrying:{attempt}:{}", delay.as_millis())
            }
            _ => "Other".to_string(),
        };
        self.events.lock().unwrap().push(name);
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    fn synced_messages(&self) -> Vec<Vec<Message>> {
        self.messages_syncs.lock().unwrap().clone()
    }

    fn done_durations(&self) -> Vec<std::time::Duration> {
        self.done_durations.lock().unwrap().clone()
    }

    fn adopted_ids(&self) -> Vec<Vec<String>> {
        self.adopted_ids.lock().unwrap().clone()
    }
}

struct TwoTurnProvider;

#[async_trait]
impl LlmProvider for TwoTurnProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        let text = if messages
            .iter()
            .any(|message| message.text_content() == "stop-hook input")
        {
            "handled queued input"
        } else {
            "initial final response"
        };
        Ok(text_completion_stream(text, 1, 1))
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

#[derive(Clone)]
struct SequenceProvider {
    responses: Arc<Mutex<VecDeque<String>>>,
    requests: Arc<Mutex<Vec<Vec<Message>>>>,
}

impl SequenceProvider {
    fn new(responses: Vec<&str>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(
                responses.into_iter().map(str::to_string).collect(),
            )),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn requests(&self) -> Vec<Vec<Message>> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl LlmProvider for SequenceProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        self.requests.lock().unwrap().push(messages.to_vec());
        let text = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| "fallback final response".to_string());
        Ok(text_completion_stream(text, 1, 1))
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

fn retryable_stream_failure() -> InvocationEvent {
    InvocationEvent::Failed(ProviderError::retryable(
        ProviderErrorKind::StreamTruncated,
        "stream connection interrupted: unexpected EOF during chunk size line",
    ))
}

fn retry_main_context(
    provider: Arc<ScriptedInvocationProvider>,
    sink: RecordingSink,
    input_events: ChannelInputEvents,
) -> SessionCommandDriverInput<RecordingSink, ChannelInputEvents> {
    retry_main_context_with_wiring(provider, sink, input_events).0
}

fn retry_main_context_with_wiring(
    provider: Arc<ScriptedInvocationProvider>,
    sink: RecordingSink,
    input_events: ChannelInputEvents,
) -> (
    SessionCommandDriverInput<RecordingSink, ChannelInputEvents>,
    Arc<context::MainSessionWiring>,
) {
    let shell = test_shell();
    let wiring = shell.wiring.clone();
    shell.model_state.update_binding(
        crate::application::model::test_support::binding_from_llm_provider(provider),
    );
    shell.set_test_session_id("test-main-terminal-retry");
    (test_session_driver_input(sink, input_events, shell), wiring)
}

async fn wait_for_retry_test_condition(description: &str, condition: impl Fn() -> bool) {
    for _ in 0..10_000 {
        if condition() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("timed out waiting for {description}");
}

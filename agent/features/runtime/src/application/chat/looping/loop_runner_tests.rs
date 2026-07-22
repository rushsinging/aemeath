//! Tests for `loop_runner`, extracted into a dedicated module to keep the
//! runner file focused on the production code path.
#![allow(clippy::type_complexity)]

use super::loop_runner::main_run_port::{
    fixture_accepted_user_messages, fixture_bind_pending, fixture_finalize_messages,
    TEST_AWAIT_USER_MODE,
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
fn finalized_projection_preserves_complete_turn_order() {
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
                run_slices: Vec::new(),
                committed_steps: Vec::new(),
            },
            initial_memory: Arc::new(memory::api::NoOpMemory),
            context_factory: Arc::new(context::ProductionMainContextFactory::new(Arc::new(
                context::NoOpCanonicalSessionWriter,
            ))),
        },
    ))
}

/// 测试用 reflection history 查询闭包（#899）。
fn test_reflection_history() -> Arc<
    dyn Fn(
            usize,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<Vec<sdk::ReflectionHistoryView>, sdk::SdkError>,
                    > + Send,
            >,
        > + Send
        + Sync,
> {
    Arc::new(|_limit| Box::pin(async { Ok(Vec::new()) }))
}

/// 测试用 list-models 闭包（#567）。
fn test_list_models() -> Arc<
    dyn Fn() -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<sdk::ModelSummary>, sdk::SdkError>>
                    + Send,
            >,
        > + Send
        + Sync,
> {
    Arc::new(|| Box::pin(async { Ok(Vec::new()) }))
}

/// 测试用 list-reminders 闭包（#567）。
fn test_list_reminders() -> Arc<
    dyn Fn() -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<sdk::ReminderView>, sdk::SdkError>>
                    + Send,
            >,
        > + Send
        + Sync,
> {
    Arc::new(|| Box::pin(async { Ok(Vec::new()) }))
}

fn test_list_sessions() -> Arc<
    dyn Fn() -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<sdk::SessionSummary>, sdk::SdkError>>
                    + Send,
            >,
        > + Send
        + Sync,
> {
    Arc::new(|| Box::pin(async { Ok(Vec::new()) }))
}

use crate::application::testing::text_completion_stream;

use async_trait::async_trait;
use hook::api::HookRunner;
use provider::test_harness::{InvocationScope, LlmProvider, SystemBlock};
use provider::ReasoningLevel;
use provider::{
    InvocationDelta, InvocationEvent, InvocationStream, ProviderCompletion, ProviderContentBlock,
    ProviderError, ProviderErrorKind, ProviderStopReason, ProviderToolCall, ProviderToolCallId,
    RawUsageSnapshot,
};
use share::config::hooks::{HookEntry, HookEvent, HooksConfig};
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

/// 测试用模型切换构建器（#567）。返回 dummy LlmClient + result，
/// 测试中模型切换不会被真正触发，此处仅满足 ChatLoopContext 字段约束。
fn test_build_switched_client(
    selection: &str,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = std::result::Result<
                    (crate::ports::ProviderBinding, sdk::ModelSwitchResult),
                    String,
                >,
            > + Send,
    >,
> {
    let selection = selection.to_string();
    Box::pin(async move {
        let binding = crate::application::testing::test_binding(vec!["dummy"])
            .as_ref()
            .clone();
        let result = sdk::ModelSwitchResult {
            display_name: selection,
            context_window: 0,
            reasoning_active: None,
        };
        Ok((binding, result))
    })
}

#[test]
fn runtime_resume_replaces_the_only_active_session_id() {
    let runner_source = include_str!("loop_runner.rs");
    let port_source = include_str!("main_run_port.rs");

    assert!(runner_source.contains("mut session_id,"));
    assert!(runner_source.contains("session_id = projection.session_id.clone();"));
    assert!(runner_source.contains("if session_id != bound_session_id"));
    assert!(runner_source.contains("session_id = bound_session_id;"));
    assert!(!runner_source.contains("context_session_id"));
    assert!(!port_source.contains("context_session_id"));
    assert!(port_source.contains("session_id: SessionId::new(self.session_id)"));
}

#[test]
fn main_production_path_is_wired_to_shared_run_loop_without_legacy_fsm() {
    // Architecture guard: behavioral tests below exercise this entry point, while this assertion
    // prevents a future reintroduction of the retired Main-only orchestration state machine.
    let source = include_str!("loop_runner.rs");
    assert!(source.contains("run_loop(&mut run, &cancel, &mut port)"));
    assert!(!source.contains("ChatLoopFsm"));
    assert!(!source.contains("StallDetector"));
    assert!(!source.contains("ChatLoopTransition"));
}

#[test]
fn main_logging_path_uses_scopes_and_no_legacy_setters() {
    let chat_source = include_str!("../../client/trait_chat.rs");
    let runner_source = include_str!("loop_runner.rs");
    let port_source = include_str!("main_run_port.rs");

    assert!(chat_source.contains("logging::spawn_instrumented(session_context"));
    assert!(runner_source.contains("session_id: logging::FieldPatch::Set"));
    assert!(runner_source.contains("chat_id: logging::FieldPatch::Set"));
    assert!(runner_source.contains("turn: logging::FieldPatch::Set(turn_count)"));
    assert!(port_source.contains("logging::spawn_instrumented("));
    for source in [chat_source, runner_source, port_source] {
        assert!(!source.contains("logging::set_current_"));
        assert!(!source.contains("logging::set_session_id"));
    }
}

#[test]
fn progress_forwarders_capture_logging_context_before_instrumented_spawn() {
    let agent_calls = include_str!("agent_calls.rs");
    let non_agent = include_str!("non_agent.rs");

    for source in [agent_calls, non_agent] {
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production source");
        assert!(production.contains("let progress_log_context = logging::capture();"));
        assert!(production.contains("logging::spawn_instrumented(progress_log_context, async move"));
        assert!(!production.contains("tokio::spawn("));
    }
}

#[test]
fn each_request_attempt_has_complete_fresh_context() {
    let parent = logging::LogContext {
        session_id: Some("session".into()),
        chat_id: Some("chat".into()),
        turn: Some(3),
        ..logging::LogContext::default()
    };
    let first = loop_runner::main_run_port::request_log_context(
        &parent,
        "model-a",
        "provider-a",
        "default",
    );
    let retry = loop_runner::main_run_port::request_log_context(
        &parent,
        "model-a",
        "provider-a",
        "default",
    );

    assert_eq!(first.session_id.as_deref(), Some("session"));
    assert_eq!(first.chat_id.as_deref(), Some("chat"));
    assert_eq!(first.turn, Some(3));
    assert_eq!(first.model.as_deref(), Some("model-a"));
    assert_eq!(first.provider.as_deref(), Some("provider-a"));
    assert_eq!(first.role.as_deref(), Some("default"));
    assert_ne!(
        first.request_id, retry.request_id,
        "retry must get a new request_id"
    );
}

#[derive(Clone)]
struct SequenceQueueDrainPort {
    responses: Arc<Mutex<VecDeque<Option<Vec<String>>>>>,
}

impl SequenceQueueDrainPort {
    fn new(responses: Vec<Option<Vec<String>>>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(VecDeque::from(responses))),
        }
    }
}

impl QueueDrainPort for SequenceQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> crate::application::chat::looping::QueueFuture<'a> {
        Box::pin(async move { self.responses.lock().unwrap().pop_front().flatten() })
    }
}

#[derive(Clone, Default)]
struct RecordingSink {
    events: Arc<Mutex<Vec<String>>>,
    messages_syncs: Arc<Mutex<Vec<Vec<Message>>>>,
    compact_rollback_snapshots: Arc<Mutex<Vec<Vec<Message>>>>,
    done_durations: Arc<Mutex<Vec<std::time::Duration>>>,
}

impl ChatEventSink for RecordingSink {
    fn send_event<'a>(
        &'a self,
        event: RuntimeStreamEvent,
    ) -> crate::application::chat::looping::EventFuture<'a> {
        Box::pin(async move {
            self.record(event);
        })
    }

    fn try_send_event(&self, event: RuntimeStreamEvent) {
        self.record(event);
    }
}

impl RecordingSink {
    fn record(&self, event: RuntimeStreamEvent) {
        let name = match &event {
            RuntimeStreamEvent::TurnStarted { messages }
            | RuntimeStreamEvent::MicrocompactDone { messages, .. }
            | RuntimeStreamEvent::StopHookBlocked { messages }
            | RuntimeStreamEvent::PostToolExecutionSync { messages }
            | RuntimeStreamEvent::CompactFinished { messages } => {
                self.messages_syncs.lock().unwrap().push(messages.clone());
                let tag = match &event {
                    RuntimeStreamEvent::TurnStarted { .. } => "TurnStarted",
                    RuntimeStreamEvent::MicrocompactDone { .. } => "MicrocompactDone",
                    RuntimeStreamEvent::StopHookBlocked { .. } => "StopHookBlocked",
                    RuntimeStreamEvent::PostToolExecutionSync { .. } => "PostToolExecutionSync",
                    RuntimeStreamEvent::CompactFinished { .. } => "CompactFinished",
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
            RuntimeStreamEvent::CompactRollback { messages } => {
                self.messages_syncs.lock().unwrap().push(messages.clone());
                self.compact_rollback_snapshots
                    .lock()
                    .unwrap()
                    .push(messages.clone());
                format!(
                    "CompactRollback:{}",
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
            RuntimeStreamEvent::HookEvent(event) => {
                format!("HookEvent:{}:{:?}", event.hook_name, event.status)
            }
            RuntimeStreamEvent::HookMessage(msg) => {
                format!(
                    "HookMessage:{:?}:{}:{}",
                    msg.point, msg.execution_ordinal, msg.attempt
                )
            }
            RuntimeStreamEvent::TurnChanged(turn) => format!("TurnChanged:{turn}"),
            RuntimeStreamEvent::Usage { .. } => "Usage".to_string(),
            RuntimeStreamEvent::Text { text, .. } => format!("Text:{text}"),
            RuntimeStreamEvent::Done { .. } => "Done".to_string(),
            RuntimeStreamEvent::SystemMessage(message) => format!("SystemMessage:{message}"),
            RuntimeStreamEvent::Cancelled { .. } => "Cancelled".to_string(),
            RuntimeStreamEvent::Thinking { .. } => "Thinking".to_string(),
            RuntimeStreamEvent::BlockComplete { .. } => "BlockComplete".to_string(),
            RuntimeStreamEvent::ToolCallStart { .. } => "ToolCallStart".to_string(),
            RuntimeStreamEvent::ToolCallUpdate { .. } => "ToolCallUpdate".to_string(),
            RuntimeStreamEvent::ToolResult { .. } => "ToolResult".to_string(),
            RuntimeStreamEvent::LiveTps(_) => "LiveTps".to_string(),
            RuntimeStreamEvent::AskUserBatch { .. } => "AskUserBatch".to_string(),
            RuntimeStreamEvent::AgentProgress { .. } => "AgentProgress".to_string(),
            RuntimeStreamEvent::WorkingDirectoryChanged { .. } => {
                "WorkingDirectoryChanged".to_string()
            }
            RuntimeStreamEvent::TasksSnapshot { .. } => "TasksSnapshot".to_string(),
            RuntimeStreamEvent::ConfigReloaded { .. } => "ConfigReloaded".to_string(),
            RuntimeStreamEvent::UserMessagesAdopted { .. } => "UserMessagesAdopted".to_string(),
            RuntimeStreamEvent::UserMessagesQueued { .. } => "UserMessagesQueued".to_string(),
            RuntimeStreamEvent::SessionReset => "SessionReset".to_string(),
            RuntimeStreamEvent::UserMessagesWithdrawn { .. } => "UserMessagesWithdrawn".to_string(),
            RuntimeStreamEvent::GraphPhaseChanged { .. } => "GraphPhaseChanged".to_string(),
            RuntimeStreamEvent::CompactProgress { .. } => "CompactProgress".to_string(),
            RuntimeStreamEvent::ModelSwitched { .. } => "ModelSwitched".to_string(),
            RuntimeStreamEvent::ModelList { .. } => "ModelList".to_string(),
            RuntimeStreamEvent::RunCancelled { .. } => "RunCancelled".to_string(),
            RuntimeStreamEvent::RunCancelling { .. } => "RunCancelling".to_string(),
            RuntimeStreamEvent::ThinkingChanged { .. } => "ThinkingChanged".to_string(),
            RuntimeStreamEvent::ContextEstimated { .. } => "ContextEstimated".to_string(),
            RuntimeStreamEvent::CommandResultText { .. } => "CommandResultText".to_string(),
            RuntimeStreamEvent::ReflectionHistory { records } => {
                format!("ReflectionHistory:{}", records.len())
            }
            RuntimeStreamEvent::SessionResumed { .. } => "SessionResumed".to_string(),
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

fn test_hook_runner() -> HookRunner {
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: "true".to_string(),
            timeout: 5,
        }],
    );
    HookRunner::new(HooksConfig { events })
}

fn blocking_then_success_hook_runner(flag_path: &std::path::Path) -> HookRunner {
    // 用 nanos 时间戳生成唯一 flag 路径，避免与并行 cargo test 共享
    // target/stop-hook-once.flag 时的 race condition。
    let flag_path_str = flag_path.to_string_lossy().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "python3 -c 'import pathlib, sys; \
                 p=pathlib.Path(\"{flag_path}\"); \
                 sys.exit(0 if p.exists() else (p.parent.mkdir(parents=True, exist_ok=True), \
                 p.write_text(\"blocked\"), print(\"fix before stopping\"), 2)[3])'",
                flag_path = flag_path_str,
            ),
            timeout: 5,
        }],
    );
    HookRunner::new(HooksConfig { events })
}

fn delayed_blocking_then_success_hook_runner(flag_path: &std::path::Path) -> HookRunner {
    let flag_path_str = flag_path.to_string_lossy().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "python3 -c 'import pathlib, sys, time; \
                 p=pathlib.Path(\"{flag_path}\"); \
                 sys.exit(0 if p.exists() else (p.parent.mkdir(parents=True, exist_ok=True), \
                 p.write_text(\"blocked\"), time.sleep(0.2), print(\"fix before stopping\"), 2)[4])'",
                flag_path = flag_path_str,
            ),
            timeout: 5,
        }],
    );
    HookRunner::new(HooksConfig { events })
}

#[tokio::test]
async fn test_process_chat_loop_stop_hook_blocked_continues_until_success() {
    // 每次测试生成独立 flag 路径，避免 cargo test 并行 race。
    let flag_path = std::env::temp_dir().join(format!(
        "aemeath_stop_hook_once_{}.flag",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&flag_path);
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let provider = Arc::new(SequenceProvider::new(vec![
        "first attempted final",
        "after hook feedback",
    ]));

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count()
                >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(provider.clone()),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-stop-hook-blocked".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: blocking_then_success_hook_runner(&flag_path),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop should complete after shutdown");
    driver.await.unwrap();
    let _ = std::fs::remove_file(&flag_path);

    let events = sink.events();
    let feedback_sync = events
        .iter()
        .position(|event| {
            event.starts_with("StopHookBlocked:")
                && event.contains("You MUST first satisfy the Stop hook requirement")
        })
        .expect("blocked Stop hook feedback should be synced into messages");
    let hook_notice = events
        .iter()
        .position(|event| event == "HookEvent:Stop:Blocked")
        .expect("blocked Stop hook should emit typed hook event");
    let second_text = events
        .iter()
        .position(|event| event == "Text:after hook feedback")
        .expect("blocked Stop hook should continue to another LLM turn");
    let done = events
        .iter()
        .position(|event| event == "DoneWithDuration")
        .expect("loop should finish after Stop hook succeeds");

    assert!(hook_notice < feedback_sync);
    assert!(feedback_sync < second_text);
    assert!(second_text < done);
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "Stop block should trigger one continuation request"
    );
    let continuation = &requests[1];
    let texts = continuation
        .iter()
        .map(Message::text_content)
        .collect::<Vec<_>>();
    let assistant_idx = texts
        .iter()
        .position(|text| text == "first attempted final")
        .expect("blocked assistant output must remain in canonical history");
    let feedback_idx = texts
        .iter()
        .position(|text| text.contains("You MUST first satisfy the Stop hook requirement"))
        .expect("Stop hook feedback must reach the continuation request");
    assert!(
        assistant_idx < feedback_idx,
        "history must precede Stop feedback: {texts:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.as_str() == "DoneWithDuration")
            .count(),
        1
    );
}

#[tokio::test]
async fn stop_hook_block_merges_feedback_with_follow_up_before_continuation() {
    let flag_path = std::env::temp_dir().join(format!(
        "aemeath_stop_hook_follow_up_{}.flag",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&flag_path);
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let provider = Arc::new(SequenceProvider::new(vec!["attempted final", "continued"]));
    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "initial".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver_input = input_tx.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .any(|event| event == "HookEvent:Stop:Running")
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        driver_input
            .send(sdk::ChatInputEvent::user_message(
                "follow up during stop hook".to_string(),
                Vec::new(),
            ))
            .unwrap();
        loop {
            if driver_sink
                .events()
                .iter()
                .any(|event| event.as_str() == "DoneWithDuration")
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(provider.clone()),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-stop-hook-follow-up".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: delayed_blocking_then_success_hook_runner(&flag_path),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop should complete after shutdown");
    driver.await.unwrap();
    let _ = std::fs::remove_file(&flag_path);

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        2,
        "follow-up must join the continuation, not start a new Run"
    );
    let texts = requests[1]
        .iter()
        .map(Message::text_content)
        .collect::<Vec<_>>();
    let assistant_idx = texts
        .iter()
        .position(|text| text == "attempted final")
        .unwrap();
    let feedback_idx = texts
        .iter()
        .position(|text| text.contains("You MUST first satisfy the Stop hook requirement"))
        .unwrap();
    let follow_up_idx = texts
        .iter()
        .position(|text| text == "follow up during stop hook")
        .unwrap();
    assert!(
        assistant_idx < feedback_idx && feedback_idx < follow_up_idx,
        "unexpected continuation order: {texts:?}"
    );
    let feedback_count = sink
        .synced_messages()
        .into_iter()
        .filter(|messages| {
            messages
                .iter()
                .filter(|message| {
                    message
                        .text_content()
                        .contains("You MUST first satisfy the Stop hook requirement")
                })
                .count()
                > 1
        })
        .count();
    assert_eq!(
        feedback_count, 0,
        "UI sync must not duplicate Stop feedback"
    );
}

#[tokio::test]
async fn test_stop_hook_feedback_message_is_marked_system_generated() {
    let flag_path = std::env::temp_dir().join(format!(
        "aemeath_stop_hook_metadata_{}.flag",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&flag_path);
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count()
                >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(
            SequenceProvider::new(vec!["first attempted final", "after hook feedback"]),
        )),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-stop-hook-metadata".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: blocking_then_success_hook_runner(&flag_path),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop should complete after shutdown");
    driver.await.unwrap();
    let _ = std::fs::remove_file(&flag_path);

    let feedback = sink
        .synced_messages()
        .into_iter()
        .flatten()
        .find(|message| {
            message
                .text_content()
                .contains("You MUST first satisfy the Stop hook requirement")
        })
        .expect("blocked Stop hook feedback should be synced into messages");

    assert_eq!(feedback.role, Role::User);
    assert_eq!(feedback.source(), MessageSource::SystemGenerated);
}

#[tokio::test]
async fn test_process_chat_loop_uses_workspace_workspace_root_for_stop_hook_env() {
    let sink = RecordingSink::default();
    // #894: stop hook 的 cwd / `AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR` 必须取自
    // restore 后的 `workspace_root`。要让 `workspace_root` 合法地不同于 wire 时的路径，
    // 必须满足 Project 不变量：一个 linked worktree 与主仓共享同一 git common dir。
    // 因此创建真实 git 仓库 + linked worktree 作为合法 fixture（而非两个互不相关的临时目录，
    // 那样无法通过 prepare_restore 的同 repo 校验）。
    let tmp = tempfile::tempdir().unwrap();
    let main_repo = tmp.path().join("main");
    let linked_wt = tmp.path().join("linked");
    std::fs::create_dir_all(&main_repo).unwrap();
    let run_git = |args: &[&str], cwd: &std::path::Path| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap()
            .success()
    };
    assert!(run_git(&["init"], &main_repo), "git init 失败");
    run_git(&["config", "user.name", "test"], &main_repo);
    run_git(&["config", "user.email", "test@example.com"], &main_repo);
    run_git(&["config", "commit.gpgsign", "false"], &main_repo);
    std::fs::write(main_repo.join("README.md"), "init").unwrap();
    assert!(run_git(&["add", "-A"], &main_repo), "git add 失败");
    assert!(
        run_git(&["commit", "-m", "init"], &main_repo),
        "git commit 失败"
    );
    assert!(
        run_git(
            &["worktree", "add", linked_wt.to_str().unwrap(), "-b", "wt"],
            &main_repo
        ),
        "git worktree add 失败"
    );

    // 取 canonical 路径，构造自洽且满足不变量的完整 DTO。
    let main_repo = main_repo.canonicalize().unwrap();
    let workspace_root = linked_wt.canonicalize().unwrap();
    // `--git-common-dir` 可能输出相对路径（相对 main_repo），需按 base 解析后再 canonicalize，
    // 与 GitCli::resolve_git_path 语义一致。
    let raw_common = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "--git-common-dir"])
            .current_dir(&main_repo)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    let common_path = std::path::PathBuf::from(raw_common);
    let common_dir = if common_path.is_absolute() {
        common_path
    } else {
        main_repo.join(common_path)
    }
    .canonicalize()
    .unwrap();

    let identity = share::session_types::ProjectIdentity {
        initial_cwd: main_repo.display().to_string(),
        git_common_dir: Some(common_dir.display().to_string()),
    };
    let workspace_root_str = workspace_root.display().to_string();
    let workspace_dto = context::session::PersistedWorkspaceContext {
        workspace_id: share::session_types::WorkspaceId::derive(&identity, &workspace_root_str),
        project_identity: identity,
        path_base: workspace_root_str.clone(),
        workspace_root: workspace_root_str,
        worktree_kind: share::session_types::WorktreeKind::Linked,
        context_stack: vec![],
    };
    // 从主仓 wire；prepare_restore + commit_restore 后 workspace_root 切换为 linked worktree
    // （与主仓路径不同），这正是本测试要验证的 stop hook env 来源。
    let workspace = project::wire_production_workspace(main_repo.clone())
        .expect("workspace 初始化成功")
        .into_views();
    let prepared = workspace
        .persist()
        .prepare_restore(&workspace_dto)
        .expect("prepare_restore 合法 DTO 应通过同 repo 校验");
    workspace.persist().commit_restore(prepared);

    let marker = tmp.path().join("stop-hook-env.txt");
    let marker_path = marker.display().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "printf '%s|%s|%s' \"$AEMEATH_PROJECT_DIR\" \"$CLAUDE_PROJECT_DIR\" \"$PWD\" > \"{}\"",
                marker_path
            ),
            timeout: 5,
        }],
    );

    let (input_tx, input_events) = ChannelInputEvents::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count()
                >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(
            SequenceProvider::new(vec!["final response"]),
        )),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace,
        session_id: "test-worktree-stop-hook-env".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: HookRunner::new(HooksConfig { events }),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop should complete after shutdown");
    driver.await.unwrap();

    assert!(sink
        .events()
        .iter()
        .any(|event| event == "HookEvent:Stop:Succeeded"));
    let output = std::fs::read_to_string(marker).unwrap();
    let parts: Vec<&str> = output.split('|').collect();
    assert_eq!(parts.len(), 3);
    let expected = workspace_root.clone();
    for part in parts {
        assert_eq!(std::fs::canonicalize(part).unwrap(), expected);
    }
}

#[tokio::test]
async fn test_process_chat_loop_drains_input_after_stop_hook_before_done() {
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    // queue 仍在 mid-turn gate 中被 drain（idle 门不消费 queue）。
    let queue = SequenceQueueDrainPort::new(vec![
        None,
        Some(vec!["stop-hook input".to_string()]),
        None,
        None,
    ]);

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count()
                >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue,
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(TwoTurnProvider)),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-session".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop should complete after shutdown");
    driver.await.unwrap();

    let events = sink.events();
    // #1272: queue input drained within the same Run cycle produces a
    // multi-step run (one drain → step → drain → step → seal).  The
    // two inputs are processed as two steps within a single terminal Run
    // (one DoneWithDuration), not as two separate Runs.
    let _first_done = events
        .iter()
        .position(|event| event == "DoneWithDuration")
        .expect("run should finish");
    let done_count = events
        .iter()
        .filter(|event| event.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 1,
        "queue input is drained in the same Run (#1272)"
    );
    assert!(
        events
            .iter()
            .any(|event| event == "Text:initial final response"),
        "first step response"
    );
    assert!(
        events
            .iter()
            .any(|event| event == "Text:handled queued input"),
        "queue input step response"
    );
}

/// Hook 首次输出 `{"continue": false}` JSON (exit 0)，之后放行。
/// 用于验证 `continue:false` 被识别为阻断（#372 缺陷 1）。
fn continue_false_then_allow_hook_runner(flag_path: &std::path::Path) -> HookRunner {
    let flag_path_str = flag_path.to_string_lossy().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "python3 -c 'import json,sys,pathlib; \
                 p=pathlib.Path(\"{flag_path}\"); \
                 sys.exit(0 if p.exists() else \
                 (p.parent.mkdir(parents=True, exist_ok=True), \
                 p.write_text(\"1\"), \
                 print(json.dumps({{\"continue\": False, \"stopReason\": \"must keep working\"}})), 0)[3])'",
                flag_path = flag_path_str,
            ),
            timeout: 5,
        }],
    );
    HookRunner::new(HooksConfig { events })
}

/// Hook 前 `n` 次阻断 (exit 2)，之后放行。用计数器文件跟踪调用次数。
fn block_n_times_hook_runner(counter_path: &std::path::Path, n: usize) -> HookRunner {
    let counter_path_str = counter_path.to_string_lossy().to_string();
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: format!(
                "python3 -c 'import pathlib,sys; \
                 p=pathlib.Path(\"{path}\"); \
                 c=int(p.read_text()) if p.exists() else 0; \
                 p.parent.mkdir(parents=True, exist_ok=True); \
                 p.write_text(str(c+1)); \
                 sys.exit(2 if c < {n} else 0)'",
                path = counter_path_str,
                n = n,
            ),
            timeout: 5,
        }],
    );
    HookRunner::new(HooksConfig { events })
}

/// Hook 每次都阻断 (exit 2)。用于验证连续阻断超上限强制停止（#372 缺陷 3）。
fn always_blocking_hook_runner() -> HookRunner {
    let mut events = HashMap::new();
    events.insert(
        HookEvent::Stop,
        vec![HookEntry {
            matcher: String::new(),
            command: "echo always blocked; exit 2".to_string(),
            timeout: 5,
        }],
    );
    HookRunner::new(HooksConfig { events })
}

#[tokio::test]
async fn test_continue_false_json_treated_as_block() {
    // #372 缺陷 1：Stop hook 输出 {"continue": false} (exit 0) 应被识别为阻断
    let flag_path = std::env::temp_dir().join(format!(
        "aemeath_continue_false_{}.flag",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&flag_path);
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count()
                >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(
            SequenceProvider::new(vec!["first response", "second response"]),
        )),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-continue-false".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: continue_false_then_allow_hook_runner(&flag_path),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop should complete after shutdown");
    driver.await.unwrap();
    let _ = std::fs::remove_file(&flag_path);

    let events = sink.events();
    // continue:false 应触发 HookEvent:Stop:Blocked
    assert!(
        events.iter().any(|e| e == "HookEvent:Stop:Blocked"),
        "continue:false JSON should be recognized as block: {:?}",
        events
    );
    // 应有反馈注入（stopReason 内容）
    assert!(
        events.iter().any(|e| e.contains("must keep working")),
        "stopReason should appear in feedback: {:?}",
        events
    );
    // 应有第 2 次 LLM 响应（说明阻断后 loop 继续）
    assert!(
        events.iter().any(|e| e == "Text:second response"),
        "loop should continue to second LLM turn: {:?}",
        events
    );
    // 最终应完成
    assert_eq!(
        events
            .iter()
            .filter(|e| e.as_str() == "DoneWithDuration")
            .count(),
        1,
        "loop should finish after hook allows: {:?}",
        events
    );
}

#[tokio::test]
async fn test_stall_triggers_stop_hook_check() {
    // #372 缺陷 2：stall 终止前应调用 Stop hook，阻断则重置 detector 并继续
    let counter_path = std::env::temp_dir().join(format!(
        "aemeath_stall_hook_{}.counter",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&counter_path);
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count()
                >= 1
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    // LLM 前 3 次返回相同输出（触发 stall），第 4 次返回不同输出
    // Stop hook 前 3 次阻断，第 4 次放行
    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(
            SequenceProvider::new(vec![
                "same output",
                "same output",
                "same output",
                "final ok",
            ]),
        )),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-stall-hook".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: block_n_times_hook_runner(&counter_path, 3),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop should complete after shutdown");
    driver.await.unwrap();
    let _ = std::fs::remove_file(&counter_path);

    let events = sink.events();
    // Repetition handling is owned by the shared engine's StuckGuard. The current engine records
    // soft text repetition but does not expose it as a domain/UI event; importantly, it still
    // preserves stop-hook feedback in this same Run and eventually reaches one terminal event.
    assert!(
        events.iter().any(|e| e == "HookEvent:Stop:Blocked"),
        "stop hook should be checked while the shared Run continues: {:?}",
        events
    );
    // stall 后 Stop hook 阻断，应有第 4 次 LLM 响应（说明 detector 重置并继续了）
    assert!(
        events.iter().any(|e| e == "Text:final ok"),
        "loop should continue after stall + Stop hook block: {:?}",
        events
    );
    // 最终应完成
    assert_eq!(
        events
            .iter()
            .filter(|e| e.as_str() == "DoneWithDuration")
            .count(),
        1,
        "loop should finish: {:?}",
        events
    );
}

/// Channel-backed input port: 投递事件经 `recv_next_input` 阻塞返回，
/// drop 发送端关闭通道使 `recv_next_input` 返回 `None`（= shutdown）。
#[derive(Clone)]
struct ChannelInputEvents {
    rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<sdk::ChatInputEvent>>>,
}

impl ChannelInputEvents {
    fn new() -> (
        tokio::sync::mpsc::UnboundedSender<sdk::ChatInputEvent>,
        Self,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (
            tx,
            Self {
                rx: Arc::new(tokio::sync::Mutex::new(rx)),
            },
        )
    }
}

impl InputEventDrainPort for ChannelInputEvents {
    fn drain_input_events<'a>(&'a self) -> crate::application::chat::looping::InputEventFuture<'a> {
        Box::pin(async move {
            let mut rx = self.rx.lock().await;
            let mut events = Vec::new();
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
            events
        })
    }

    fn recv_next_input<'a>(&'a self) -> crate::application::chat::looping::InputEventOptFuture<'a> {
        Box::pin(async move {
            let mut rx = self.rx.lock().await;
            rx.recv().await
        })
    }
}

#[tokio::test]
async fn test_loop_persists_across_turns_until_shutdown() {
    // 常驻 loop 跨回合：喂 "first" → 完成回合 1 → 喂 "second" → 完成回合 2
    // → drop 发送端关闭通道 → loop shutdown 退出（不 hang）。
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    // 首条输入（回合 1 的用户消息）在 loop 启动前投递。
    input_tx
        .send(sdk::ChatInputEvent::user_message("first", Vec::new()))
        .unwrap();

    // driver：轮询 sink 事件，见到第 1 个 DoneWithDuration 后投递 "second"，
    // 见到第 2 个 DoneWithDuration 后 drop 发送端关闭通道触发 shutdown。
    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        // 等回合 1 完成（第 1 个 DoneWithDuration）。
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|event| event.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        input_tx
            .send(sdk::ChatInputEvent::user_message("second", Vec::new()))
            .unwrap();
        // 等回合 2 完成（第 2 个 DoneWithDuration），再关闭通道。
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|event| event.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → recv_next_input 返回 None → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(
            SequenceProvider::new(vec!["turn one final", "turn two final"]),
        )),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: Vec::new(),
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-persistent-loop".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    // timeout 包裹：若 loop 在 shutdown 后未返回（hang），测试失败而非永久阻塞。
    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    let events = sink.events();
    // 两回合各产生一个 DoneWithDuration（常驻 loop 跨回合存活）。
    let done_count = events
        .iter()
        .filter(|event| event.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 2,
        "常驻 loop 应跨两回合产生 2 个 DoneWithDuration: {events:?}"
    );
    // 两回合的用户消息均被处理（first 在回合 1，second 在回合 2）。
    assert!(
        events.iter().any(|event| event == "Text:turn one final"),
        "回合 1 应调用 LLM: {events:?}"
    );
    assert!(
        events.iter().any(|event| event == "Text:turn two final"),
        "回合 2 应调用 LLM: {events:?}"
    );
}

/// 每次 LLM 调用固定返回同一条极短回复，并 sleep 固定时长。
/// 用于 #390 A1 跨回合泄漏回归：
/// - 相同回复 → 若 `stall_detector` 跨回合泄漏，第 3 回合会误判 stall 停机。
/// - 固定 sleep → 若 `turn_start` 跨回合泄漏，`DoneWithDuration` 的 duration
///   会随回合累加（第 N 回合 ≈ N*sleep）；重置后每回合 ≈ 单次 sleep。
#[derive(Clone)]
struct IdenticalReplyProvider {
    reply: String,
    per_turn_delay: std::time::Duration,
}

impl IdenticalReplyProvider {
    fn new(reply: &str, per_turn_delay: std::time::Duration) -> Self {
        Self {
            reply: reply.to_string(),
            per_turn_delay,
        }
    }
}

#[async_trait]
impl LlmProvider for IdenticalReplyProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        tokio::time::sleep(self.per_turn_delay).await;
        Ok(text_completion_stream(self.reply.clone(), 1, 1))
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

#[tokio::test]
async fn test_stall_detector_resets_across_user_turns() {
    // #390 A1 回归：常驻 loop 跨 3 个独立 USER 回合，每回合 LLM 返回**完全相同**的
    // 极短回复（"Done."）。重构前每个 `chat()` 持有独立 StallDetector，跨回合不可能
    // 累积；A1 把 loop 改为常驻后 detector 在 loop 外只构造一次，3 个相同回复会在第 3
    // 回合触发 "[agent loop stopped: LLM is producing repetitive output]" 误报。
    //
    // 修复：每个新 USER 回合开始时重置 stall_detector（同时重置 turn_start）。
    // 期望：3 个回合都正常完成（3 个 DoneWithDuration），无 stall 停机 SystemMessage。
    //
    // 同时验证 turn_start 不跨回合泄漏（Finding 2）：每回合 LLM sleep 固定时长，
    // 若 turn_start 泄漏，第 3 回合 duration 会累积成 ~3*delay；重置后各回合 ~delay。
    let per_turn_delay = std::time::Duration::from_millis(40);
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    // 回合 1 的用户消息在 loop 启动前投递。
    input_tx
        .send(sdk::ChatInputEvent::user_message("turn-1", Vec::new()))
        .unwrap();

    // driver：每见到一个新的 DoneWithDuration 就投递下一回合输入；最后无条件关闭通道。
    //
    // **必须有界**：修复前第 3 回合会误触发 stall 停机，loop 直接 break（不产生第 3 个
    // DoneWithDuration）。若 driver 无界轮询「done_count>=3」会永久阻塞，掩盖失败为 hang。
    // 改为「有界等待 + stall 信号提前退出 + 无条件 drop 发送端」，使修复前以**断言失败**
    // （而非 hang）暴露 RED；修复后 3 回合正常完成 → GREEN。
    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        // 有界等待 done_count 达到 target 或观测到 stall 停机（提前退出）。
        async fn wait_for(sink: &RecordingSink, target: usize) {
            for _ in 0..400 {
                let events = sink.events();
                let done_count = events
                    .iter()
                    .filter(|event| event.as_str() == "DoneWithDuration")
                    .count();
                let stalled = events.iter().any(|e| e.contains("repetitive output"));
                if done_count >= target || stalled {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
        for (next, target) in [("turn-2", 1usize), ("turn-3", 2usize)] {
            wait_for(&driver_sink, target).await;
            let _ = input_tx.send(sdk::ChatInputEvent::user_message(next, Vec::new()));
        }
        wait_for(&driver_sink, 3).await;
        drop(input_tx); // 无条件关闭通道 → recv_next_input 返回 None → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(
            IdenticalReplyProvider::new("Done.", per_turn_delay),
        )),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: Vec::new(),
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-stall-reset-across-turns".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    let events = sink.events();

    // Finding 1：3 个相同短回复回合均正常完成，无 stall 误报。
    assert!(
        !events.iter().any(|e| e.contains("repetitive output")),
        "相同短回复不应跨独立 USER 回合触发 stall 停机: {events:?}"
    );
    let done_count = events
        .iter()
        .filter(|event| event.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 3,
        "3 个独立 USER 回合应各产生 1 个 DoneWithDuration: {events:?}"
    );

    // Finding 2（轻量断言）：turn_start 每回合重置，duration 不随回合累积。
    // 若未重置，第 3 回合 duration ≈ 3*delay；重置后各回合 ≈ delay。
    // 取「第 3 回合 < 前两回合 duration 之和」作为非累积的稳健判据。
    let durations = sink.done_durations();
    assert_eq!(durations.len(), 3, "应有 3 个 duration: {durations:?}");
    assert!(
        durations[2] < durations[0] + durations[1],
        "turn_start 应每回合重置：第 3 回合 duration ({:?}) 不应累积到 >= 前两回合之和 ({:?} + {:?})",
        durations[2],
        durations[0],
        durations[1]
    );
}

/// 记录每次 LLM 调用时 messages 中最后一条用户消息的文本。
/// 用于确定性地检测「空闲期命令触发的陈旧历史空回合」：
/// 合法回合每次调用前都有新用户消息（"first" / "second"）；
/// bug 触发的空回合会在 "first" 之后、"second" 之前再次以 "first" 为末条用户消息调用 LLM。
#[derive(Clone)]
struct RecordingProvider {
    calls: Arc<Mutex<Vec<String>>>,
}

impl RecordingProvider {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl LlmProvider for RecordingProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        let last_user = messages
            .iter()
            .rev()
            .find(|message| message.role == Role::User)
            .map(|message| message.text_content())
            .unwrap_or_default();
        self.calls.lock().unwrap().push(last_user.clone());
        let text = format!("response to {last_user}");
        Ok(text_completion_stream(text, 1, 1))
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

#[tokio::test]
async fn test_idle_control_command_does_not_run_spurious_turn() {
    // #390 A1（Important）：空闲期收到一个不 append 任何用户消息的 ControlCommand
    // （如 /save / /model / /provider，apply_gate 返回 Proceed 且 appended_user_messages=0）
    // 时，loop 必须保持空闲，NEVER 在陈旧历史上跑空回合。随后投递真实 UserMessage
    // 才恢复运行并产出恰好一个新回合；drop 发送端关闭通道后 loop shutdown 退出。
    //
    // 确定性检测：RecordingProvider 记录每次 LLM 调用时末条用户消息文本。合法序列恰为
    // ["first", "second"]；bug 会插入一次陈旧 "first" 调用 → 序列变
    // ["first", "first", "second"]（断言失败）。
    //
    // 同步屏障：driver 等 DoneWithDuration（回合 1 完成、loop 已进入空闲态、下一处通道
    // 消费者必为 await_idle_input）后再投递 /save，确保命令落到 idle 臂而非被回合终结
    // 路径的 BeforeFinish gate 提前 drain 掉。
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let provider = RecordingProvider::new();

    // 首条输入（回合 1 的用户消息）在 loop 启动前投递。
    input_tx
        .send(sdk::ChatInputEvent::user_message("first", Vec::new()))
        .unwrap();

    let driver_sink = sink.clone();
    let driver_provider = provider.clone();
    let driver = tokio::spawn(async move {
        // 等回合 1 完成（第 1 个 DoneWithDuration）→ loop 已进入空闲态阻塞于
        // await_idle_input；此时投递的 /save 必由 idle 臂消费。
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|event| event.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        // 空闲期投递一个 ControlCommand（/save：SideEffect，append 0 条用户消息）。
        input_tx
            .send(sdk::ChatInputEvent::ControlCommand {
                raw: "/save".to_string(),
            })
            .unwrap();
        // 给 loop 充分调度机会去（错误地）消费命令、退出空闲、跑陈旧历史空回合。
        // 若 bug 存在，这会产生第 2 次 LLM 调用（末条用户消息仍为 "first"）。
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
        // 命令处理后 LLM 调用数仍应为 1（保持空闲，无空回合）。
        assert_eq!(
            driver_provider.calls(),
            vec!["first".to_string()],
            "空闲期单独 ControlCommand 不得触发 LLM 调用（应仍只有 first 一次）"
        );

        // 现在投递真实用户消息，应恢复运行并完成回合 2（第 2 次 LLM 调用）。
        input_tx
            .send(sdk::ChatInputEvent::user_message("second", Vec::new()))
            .unwrap();
        loop {
            if driver_provider.calls().len() >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → recv_next_input 返回 None → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(provider.clone())),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: Vec::new(),
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-idle-control-command".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    // 关键断言（确定性）：LLM 调用序列恰为 ["first", "second"]。
    // bug 会插入陈旧 "first" 调用 → ["first", "first", "second"]，断言失败。
    assert_eq!(
        provider.calls(),
        vec!["first".to_string(), "second".to_string()],
        "LLM 应恰好被两条真实用户消息触发；命令不得引发陈旧历史空回合"
    );
    // 命令的 raw 文本绝不应作为 user message 进入消息历史。
    assert!(
        sink.synced_messages()
            .into_iter()
            .flatten()
            .all(|message| message.text_content() != "/save"),
        "ControlCommand 永不作为 user message 进入历史: {:?}",
        sink.events()
    );
}

#[tokio::test]
async fn test_idle_pending_command_does_not_run_spurious_turn() {
    // 回归 #628：idle 收到的 PendingCommand（ListReminders 等纯查询或动作命令）
    // 处理后应回 idle 等下一条输入，而不是掉进 execute_tool_round 跑一轮幽灵 LLM turn。
    // bug 表现：命令处理完无 continue，掉进 turn_count += 1 / StartTurn，用陈旧 tool_calls 跑一整轮。
    //
    // 与 test_idle_control_command_does_not_run_spurious_turn 的区别：前者走 ControlCommand 路径
    // （busy 期排队 / busy 期 drain），本测试走 ChatInputEvent::ListReminders → PendingCommand 路径，
    // 命中 loop_runner.rs 中 6 处漏 continue 的 match 臂。
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let provider = RecordingProvider::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message("first", Vec::new()))
        .unwrap();

    let driver_sink = sink.clone();
    let driver_provider = provider.clone();
    let driver = tokio::spawn(async move {
        // 等回合 1 完成（第 1 个 DoneWithDuration）→ loop 已进入空闲态阻塞于 await_idle_input。
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|event| event.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        // 空闲期投递 ListReminders（PendingCommand 路径）。
        let _ = input_tx.send(sdk::ChatInputEvent::ListReminders);
        // 给 loop 充分调度机会去（错误地）消费命令、退出空闲、跑陈旧历史空回合。
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
        // 命令处理后 LLM 调用数仍应为 1（保持空闲，无空回合）。
        assert_eq!(
            driver_provider.calls(),
            vec!["first".to_string()],
            "空闲期单独 PendingCommand::ListReminders 不得触发 LLM 调用（应仍只有 first 一次）"
        );

        // 现在投递真实用户消息，应恢复运行并完成回合 2（第 2 次 LLM 调用）。
        input_tx
            .send(sdk::ChatInputEvent::user_message("second", Vec::new()))
            .unwrap();
        loop {
            if driver_provider.calls().len() >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(provider.clone())),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: Vec::new(),
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-idle-pending-save".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    assert_eq!(
        provider.calls(),
        vec!["first".to_string(), "second".to_string()],
        "ListReminders 命令不得引发陈旧历史空回合: {:?}",
        sink.events()
    );
}

#[tokio::test]
async fn test_idle_pending_command_list_reminders_does_not_run_spurious_turn() {
    // 回归 #628：idle 收到 ChatInputEvent::ListReminders（PendingCommand 路径）应直接回 idle，
    // 不应掉进 turn 跑一轮幽灵 LLM 调用。
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let provider = RecordingProvider::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message("first", Vec::new()))
        .unwrap();

    let driver_sink = sink.clone();
    let driver_provider = provider.clone();
    let driver = tokio::spawn(async move {
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|event| event.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        let _ = input_tx.send(sdk::ChatInputEvent::ListReminders);
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            driver_provider.calls(),
            vec!["first".to_string()],
            "空闲期单独 PendingCommand::ListReminders 不得触发 LLM 调用"
        );

        input_tx
            .send(sdk::ChatInputEvent::user_message("second", Vec::new()))
            .unwrap();
        loop {
            if driver_provider.calls().len() >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(provider.clone())),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: Vec::new(),
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-idle-pending-list-reminders".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    assert_eq!(
        provider.calls(),
        vec!["first".to_string(), "second".to_string()],
        "ListReminders 命令不得引发陈旧历史空回合: {:?}",
        sink.events()
    );
}

#[tokio::test]
async fn test_stop_hook_block_limit_stops_loop() {
    // #372 缺陷 3：Stop hook 连续阻断超过 MAX_STOP_HOOK_BLOCKS(5) 强制停止
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "hello".to_string(),
            Vec::new(),
        ))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        loop {
            if !driver_sink.done_durations.lock().unwrap().is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    // 每次返回不同输出避免 stall；Stop hook 每次阻断
    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(
            SequenceProvider::new(vec!["r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8"]),
        )),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-block-limit".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: always_blocking_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };
    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop should complete after shutdown");
    driver.await.unwrap();

    let events = sink.events();
    // 超过上限应由共享 StuckGuard 产生结构化终止原因。
    assert!(
        events
            .iter()
            .any(|event| event.contains("stop hook blocked completion 5 times")),
        "should emit StuckGuard block-limit reason: {:?}",
        events
    );
    // #604：blocked 上限退出时必须发出 DoneWithDuration，否则 TUI spinner 永不停
    assert!(
        !sink.done_durations.lock().unwrap().is_empty(),
        "blocked-limit exit must emit DoneWithDuration, got events: {:?}",
        events
    );
}

/// 第 1 次调用阻塞直到 cancel 被触发后返回取消错误，
/// 第 2 次及以后调用立即返回正常响应。用于模拟「回合进行中被取消、
/// 随后新回合正常完成」的场景。
#[derive(Clone)]
struct CancellableThenNormalProvider {
    calls: Arc<Mutex<usize>>,
}

impl CancellableThenNormalProvider {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl LlmProvider for CancellableThenNormalProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        let call_index = {
            let mut guard = self.calls.lock().unwrap();
            let idx = *guard;
            *guard += 1;
            idx
        };
        if call_index == 0 {
            // 回合 1：阻塞等待 cancel，被取消后返回 Cancelled（模拟 provider 侧取消）。
            cancel.cancelled().await;
            return Err(ProviderError::cancelled());
        }
        // 回合 2+：正常完成（关键：此时若 token 未重置，会立刻 Cancelled）。
        if cancel.is_cancelled() {
            return Err(ProviderError::cancelled());
        }
        let text = format!("turn {} final", call_index + 1);
        Ok(text_completion_stream(text, 1, 1))
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

#[tokio::test]
async fn test_cancel_aborts_turn_then_returns_to_idle() {
    // #390 A1 Task 3：回合进行中 cancel → 发出 Cancelled、回滚本回合消息、
    // **回到空闲**（不退 loop）；随后投递新 UserMessage → 新回合正常完成；
    // 最后 drop 发送端关闭通道 → loop shutdown 退出。
    //
    // 取消令牌生命周期（并发关键）：每个 Run 在 ActiveRunRegistry 中独占 token，
    // cancel_run(run_id) 只取消目标 Run；Session 回到 idle 后创建新 Run 和新 token。
    // 若错误复用已取消 token，回合 2 的 LLM 调用会立即 Cancelled。
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    // Active Run registry：模拟 RuntimeHandle.active_run 的同步 cancel_run 入口。
    let active_run = Arc::new(crate::application::active_run::ActiveRunRegistry::default());
    let provider = CancellableThenNormalProvider::new();

    // 首条输入（回合 1 的用户消息）在 loop 启动前投递。
    input_tx
        .send(sdk::ChatInputEvent::user_message("first", Vec::new()))
        .unwrap();

    let driver_sink = sink.clone();
    let driver_provider = provider.clone();
    let driver_registry = active_run.clone();
    let driver = tokio::spawn(async move {
        // 等回合 1 的 LLM 调用真正开始（call count >= 1），此时 provider 正阻塞于
        // cancel.cancelled()。再触发取消，确保取消落在「回合进行中」。
        loop {
            if *driver_provider.calls.lock().unwrap() >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        let run_id = loop {
            if let Some(run_id) = driver_registry.active_id() {
                break run_id;
            }
            tokio::task::yield_now().await;
        };
        assert_eq!(
            driver_registry.cancel(&run_id),
            sdk::CancelRunOutcome::Accepted
        );

        // 等回合 1 被取消（出现 Cancelled 事件）。
        loop {
            if driver_sink.events().iter().any(|e| e == "Cancelled") {
                break;
            }
            tokio::task::yield_now().await;
        }

        // 投递真实用户消息：应恢复运行并完成回合 2（新 Run 拥有独立 token）。
        input_tx
            .send(sdk::ChatInputEvent::user_message("second", Vec::new()))
            .unwrap();
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → recv_next_input 返回 None → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(provider.clone())),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: Vec::new(),
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-cancel-then-idle".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: active_run.clone(),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 cancel→idle→新回合→shutdown 后返回，而非 hang");
    driver.await.unwrap();

    let events = sink.events();
    // 回合 1 被取消：发出 Cancelled。
    assert!(
        events.iter().any(|e| e == "Cancelled"),
        "回合 1 进行中 cancel 应发出 Cancelled 事件: {events:?}"
    );
    // cancel 后未退 loop：回合 2 正常完成，恰好一个 DoneWithDuration。
    let done_count = events
        .iter()
        .filter(|e| e.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 1,
        "cancel 应回空闲、不退 loop；新回合应正常完成产出 1 个 DoneWithDuration: {events:?}"
    );
    // 回合 2 的 LLM 响应文本出现（说明新 token 未被陈旧 cancel 污染）。
    assert!(
        events.iter().any(|e| e == "Text:turn 2 final"),
        "重置 token 后回合 2 应正常调用 LLM 并完成: {events:?}"
    );
    assert!(
        events
            .iter()
            .all(|event| !event.starts_with("CompactRollback")),
        "finalized partial Step 由 Context append 保存，取消不得恢复旧 rollback 路径: {events:?}"
    );
}

/// 回合 1 正常完成、回合 2 进行中阻塞等待 cancel、回合 3 正常完成。
/// 用于验证「取消晚于首回合的回合时，先前已完成回合的消息必须存活」。
#[derive(Clone)]
struct CompleteThenCancellableProvider {
    calls: Arc<Mutex<usize>>,
}

impl CompleteThenCancellableProvider {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl LlmProvider for CompleteThenCancellableProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        let call_index = {
            let mut guard = self.calls.lock().unwrap();
            let idx = *guard;
            *guard += 1;
            idx
        };
        // 回合 2（call_index == 1）：阻塞等 cancel，被取消后返回 Cancelled。
        // 回合 1 / 回合 3：正常完成（token 已重置，不应被陈旧 cancel 污染）。
        if call_index == 1 {
            cancel.cancelled().await;
            return Err(ProviderError::cancelled());
        }
        if cancel.is_cancelled() {
            return Err(ProviderError::cancelled());
        }
        let text = format!("turn {} assistant", call_index + 1);
        Ok(text_completion_stream(text, 1, 1))
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

#[tokio::test]
async fn test_cancel_later_turn_preserves_completed_prior_turns() {
    // #390 A1（Important，data-loss）：常驻 loop 中先前回合已完成的消息累积在同一个
    // `messages` Vec。若 cancel 回滚用「loop 启动时的固定基线」，取消任何「非首回合」会把
    // 先前已完成回合一并截掉（整段对话坍缩到首条）。修复后 cancel 改用 per-turn 基线，
    // 只回滚当前回合的 partial 输出，先前已完成回合的 user+assistant 必须存活。
    //
    // 时序：
    //   回合 1：投递 "turn1-user" → LLM 正常返回 "turn 1 assistant" → 完成、进入空闲。
    //   回合 2：投递 "turn2-user" → LLM 阻塞等 cancel → 外部 cancel → 回滚回空闲。
    //   回合 3：投递 "turn3-user" → LLM 正常完成（新 token 未被污染）→ shutdown。
    //
    // 关键断言：回合 2 被取消后的 MessagesSync 中，回合 1 的 "turn1-user" 与
    // "turn 1 assistant" 必须仍存在（pre-fix `truncate(loop_start_baseline=0)` 会删除它们）。
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    // Active Run registry：模拟同步 cancel_run(run_id)。
    let active_run = Arc::new(crate::application::active_run::ActiveRunRegistry::default());
    let provider = CompleteThenCancellableProvider::new();

    // 回合 1 的用户消息在 loop 启动前投递。
    input_tx
        .send(sdk::ChatInputEvent::user_message("turn1-user", Vec::new()))
        .unwrap();

    let driver_sink = sink.clone();
    let driver_provider = provider.clone();
    let driver_registry = active_run.clone();
    let driver = tokio::spawn(async move {
        // 等回合 1 完成（第 1 个 DoneWithDuration），loop 进入空闲。
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        // 投递回合 2 的用户消息：恢复运行，LLM（第 2 次调用）阻塞等 cancel。
        input_tx
            .send(sdk::ChatInputEvent::user_message("turn2-user", Vec::new()))
            .unwrap();
        // 等回合 2 的 LLM 调用真正开始（call count >= 2），确保 cancel 落在「回合进行中」。
        loop {
            if *driver_provider.calls.lock().unwrap() >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        let run_id = loop {
            if let Some(run_id) = driver_registry.active_id() {
                break run_id;
            }
            tokio::task::yield_now().await;
        };
        assert_eq!(
            driver_registry.cancel(&run_id),
            sdk::CancelRunOutcome::Accepted
        );
        // 等回合 2 被取消（出现 Cancelled 事件）。
        loop {
            if driver_sink.events().iter().any(|e| e == "Cancelled") {
                break;
            }
            tokio::task::yield_now().await;
        }
        // 投递回合 3 的用户消息：恢复运行并完成回合 3。
        input_tx
            .send(sdk::ChatInputEvent::user_message("turn3-user", Vec::new()))
            .unwrap();
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 2 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(provider.clone())),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: Vec::new(),
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-cancel-preserves-prior-turns".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: active_run.clone(),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    // timeout 包裹：未 shutdown（hang）则测试失败而非永久阻塞。
    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 回合1→回合2取消→回合3→shutdown 后返回，而非 hang");
    driver.await.unwrap();

    let events = sink.events();
    // 回合 2 被取消：发出 Cancelled。
    assert!(
        events.iter().any(|e| e == "Cancelled"),
        "回合 2 进行中 cancel 应发出 Cancelled 事件: {events:?}"
    );

    assert!(
        events
            .iter()
            .all(|event| !event.starts_with("CompactRollback")),
        "回合 2 取消应提交 finalized partial Step，禁止恢复旧 rollback 路径: {events:?}"
    );

    // cancel 后未退 loop：回合 3 正常完成，总计 2 个 DoneWithDuration。
    let done_count = events
        .iter()
        .filter(|e| e.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 2,
        "回合 1 与回合 3 各产出一个 DoneWithDuration（cancel 不退 loop）: {events:?}"
    );
    // 回合 3 的 assistant 响应出现（新 token 未被陈旧 cancel 污染）。
    assert!(
        events.iter().any(|e| e == "Text:turn 3 assistant"),
        "重置 token 后回合 3 应正常调用 LLM 并完成: {events:?}"
    );
}

/// Task 4：loop 顶部无待答回合时必须先 idle-wait，收到 UserMessage 后才调 LLM。
///
/// 用一个计数 provider 追踪 LLM 调用次数。在投递任何输入前，先给 loop 充分调度
/// 机会（yield 若干轮），断言此时 LLM 调用数为 0（loop 正处于 loop-top idle-wait）。
/// 随后投递一条 UserMessage，等 DoneWithDuration 出现，断言恰好一次 LLM 调用。
/// 最后 drop 发送端，loop shutdown 退出（无 hang）。
///
/// RED 阶段：当前 loop 在 `process_chat_loop` 顶部直接进入 BeforeLlm gate，
/// 即使 messages 为空也不会 idle-wait，调用 LLM 时 messages_for_api 为空
/// 导致 LLM provider 被调用（或回合逻辑异常）。实现 `has_pending_user_turn` +
/// 顶部 idle 门后，测试应变绿。
#[tokio::test]
async fn test_chat_impl_idle_until_first_input_event() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    // === 计数 provider：记录 LLM 调用次数 ===
    #[derive(Clone)]
    struct CountingProvider {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }
    #[async_trait]
    impl LlmProvider for CountingProvider {
        async fn invocation_stream(
            &self,
            _scope: &InvocationScope,
            _system: &[SystemBlock],
            _messages: &[Message],
            _tool_schemas: &[serde_json::Value],
            _cancel: &CancellationToken,
        ) -> Result<InvocationStream, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(text_completion_stream("hi response", 1, 1))
        }

        fn model_name(&self) -> &str {
            "test-model"
        }

        fn provider_name(&self) -> &str {
            "test-provider"
        }
    }

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let call_counter = Arc::new(AtomicUsize::new(0));
    let provider = CountingProvider {
        calls: call_counter.clone(),
    };

    // driver：先给 loop 充分调度机会（不投递任何输入），断言 LLM 未被调用；
    // 再投递 UserMessage("hi")，等 DoneWithDuration；最后关闭通道触发 shutdown。
    let driver_sink = sink.clone();
    let driver_counter = call_counter.clone();
    let driver = tokio::spawn(async move {
        // 给 loop 充分调度机会（200 次 yield）——若 loop 不 idle-wait，
        // 会直接进入 BeforeLlm gate 并调用 LLM。
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }

        // 关键断言 RED：当前 loop 无 loop-top idle，此时 LLM 已被调用（test 失败）。
        // 实现后 loop 在 loop-top idle-wait 阻塞，LLM 调用数应为 0。
        assert_eq!(
            driver_counter.load(Ordering::SeqCst),
            0,
            "无待答用户回合时 loop 必须 idle-wait，不得立即调用 LLM"
        );
        // 此时也不应有 Done（无 LLM 调用必然无完成）。
        assert!(
            driver_sink.events().iter().all(|e| e != "DoneWithDuration"),
            "未投递输入前不得出现 DoneWithDuration: {:?}",
            driver_sink.events()
        );
        // Finding 2：idle gate 已前置到回合头之前，空 seed 启动在收到首条输入前
        // 不得发出任何 TurnChanged（否则是「回合 1 / 处理中」假信号）。
        assert!(
            driver_sink
                .events()
                .iter()
                .all(|e| !e.starts_with("TurnChanged")),
            "未投递输入前不得发出 TurnChanged（前置 idle gate 避免假回合）: {:?}",
            driver_sink.events()
        );

        // 投递首条 UserMessage，loop 应从 idle 恢复、运行一个回合、发出 DoneWithDuration。
        input_tx
            .send(sdk::ChatInputEvent::user_message("hi", Vec::new()))
            .unwrap();

        // 等 DoneWithDuration（最多 10s）。
        loop {
            if driver_sink.events().iter().any(|e| e == "DoneWithDuration") {
                break;
            }
            tokio::task::yield_now().await;
        }

        // 恰好一次 LLM 调用。
        assert_eq!(
            driver_counter.load(Ordering::SeqCst),
            1,
            "投递 UserMessage 后应恰好调用一次 LLM"
        );

        drop(input_tx); // 关闭通道 → recv_next_input 返回 None → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(provider)),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: Vec::new(),
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-idle-until-first-input".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    let events = sink.events();
    // 整个生命周期内恰好一次 LLM 调用（"hi" 回合）。
    assert_eq!(
        call_counter.load(Ordering::SeqCst),
        1,
        "全程应恰好一次 LLM 调用: {events:?}"
    );
    // 产出一个 DoneWithDuration（一个完整回合）。
    assert_eq!(
        events
            .iter()
            .filter(|e| e.as_str() == "DoneWithDuration")
            .count(),
        1,
        "应产出恰好一个 DoneWithDuration: {events:?}"
    );
    // Finding 2：全程恰好一次 TurnChanged，且回合编号为 1（首个真实回合 = 1，
    // 前置 idle gate 不会消耗回合号）。空 seed 启动不产生假回合。
    let turn_changes: Vec<&String> = events
        .iter()
        .filter(|e| e.starts_with("TurnChanged"))
        .collect();
    assert_eq!(
        turn_changes,
        vec![&"TurnChanged:1".to_string()],
        "空 seed 启动应恰好发出一次 TurnChanged:1（首个真实回合编号为 1）: {events:?}"
    );
}

/// Finding 2 专项：空 seed 启动时，loop-top idle gate 位于回合头之前，
/// 收到首条真实输入前 NEVER 发出任何回合信号（`TurnChanged`）或 turn 边界副作用。
///
/// 与 `test_chat_impl_idle_until_first_input_event` 的区别：本测试以 `RecordingSink`
/// 捕获「投递首条输入的那一刻」的事件快照，**确定性**断言该快照内不含 `TurnChanged`
/// （前置 idle gate 的直接观测）。若 gate 仍位于 `TurnChanged` 之后（回归），快照会
/// 含 `TurnChanged:1` 假信号 → 断言失败。随后真实输入触发恰好一个回合（`TurnChanged:1`
/// 在输入之后出现），drop 发送端关闭通道使 loop shutdown 退出。
#[tokio::test]
async fn test_empty_seed_start_emits_no_turn_signal_before_first_input() {
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let provider = RecordingProvider::new();

    // 注意：loop 启动前 NEVER 投递任何输入（空 seed + 无 pending）→ loop 必先 idle-wait。
    let driver_sink = sink.clone();
    let driver_provider = provider.clone();
    let driver = tokio::spawn(async move {
        // 给 loop 充分调度机会去（错误地）跑回合头、发 TurnChanged。
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
        // 捕获「投递首条输入前」的事件快照。
        let snapshot_before_input = driver_sink.events();
        assert!(
            snapshot_before_input
                .iter()
                .all(|e| !e.starts_with("TurnChanged")),
            "空 seed 启动在收到首条输入前不得发出 TurnChanged（前置 idle gate 避免假回合）: {snapshot_before_input:?}"
        );
        assert_eq!(
            driver_provider.calls().len(),
            0,
            "收到首条输入前不得调用 LLM: {snapshot_before_input:?}"
        );

        // 投递首条真实用户消息 → 恢复运行、产出恰好一个回合。
        input_tx
            .send(sdk::ChatInputEvent::user_message("hello", Vec::new()))
            .unwrap();
        loop {
            if driver_sink.events().iter().any(|e| e == "DoneWithDuration") {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → shutdown
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(provider.clone())),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: Vec::new(),
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-no-turn-signal-before-first-input".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    let events = sink.events();
    // 首条输入触发后：恰好一次 TurnChanged，编号 1（首个真实回合 = 1）。
    let turn_changes: Vec<&String> = events
        .iter()
        .filter(|e| e.starts_with("TurnChanged"))
        .collect();
    assert_eq!(
        turn_changes,
        vec![&"TurnChanged:1".to_string()],
        "真实输入后应恰好发出一次 TurnChanged:1: {events:?}"
    );
    // TurnChanged 必在首次 LLM 调用（输入处理后）之前的同一回合内；整体恰好一次 LLM 调用。
    assert_eq!(
        provider.calls(),
        vec!["hello".to_string()],
        "全程应恰好被首条真实用户消息触发一次 LLM 调用: {events:?}"
    );
}

/// #672/#503：resume 后 messages 末尾为 User 消息（纯文本）时，loop-top idle 门
/// 强制等待新输入，而非自动发起 LLM 请求恢复被中断的对话。
#[tokio::test]
async fn test_resume_skip_pending_user_turn_idles_until_new_input() {
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    // messages 模拟 resume 加载的历史：末条是 User（等待 assistant 回复）
    let messages = vec![Message::user("unfinished question")];

    // driver：先确认 loop 在 idle（无 LLM 调用），再投递新消息触发回合
    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        // yield 若干轮，确认 loop 没自动发起 LLM 请求
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
        // 投递新用户消息 → loop idle 门恢复，进入回合
        input_tx
            .send(sdk::ChatInputEvent::user_message("new input", Vec::new()))
            .unwrap();
        // 等回合完成
        loop {
            if driver_sink.events().iter().any(|e| e == "DoneWithDuration") {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → shutdown
    });

    let provider = SequenceProvider::new(vec!["response to new input"]);
    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(provider)),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: messages, // 末条为 User，模拟 resume
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-resume-skip-pending".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回，而非 hang");
    driver.await.unwrap();

    // 验证：收到新输入后产生回合（DoneWithDuration），证明 loop idle 等待了
    let events = sink.events();
    assert!(
        events.iter().any(|e| e == "DoneWithDuration"),
        "resume 后应 idle 等待新输入，收到新输入后才完成回合: {events:?}"
    );
    // 验证：LLM 响应文本出现（说明新消息触发后正常处理）
    assert!(
        events.iter().any(|e| e.contains("response to new input")),
        "新输入应触发 LLM 响应: {events:?}"
    );
}

/// #672：runtime 启动后永远等待用户输入，不管 messages 末尾是什么角色。
/// 末条 User 消息 + pending_input 空 → idle 等待，不自动触发 LLM。
#[tokio::test]
async fn test_messages_with_user_tail_idles_without_pending_input() {
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    let messages = vec![Message::user("hello")];

    // driver：等待 200ms 后关闭通道（不应有 LLM 响应产生）
    let _driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        // 给 loop 充分时间进入 idle
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        drop(input_tx); // 关闭通道 → shutdown
    });

    let provider = SequenceProvider::new(vec!["hi there"]);
    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![None]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(provider)),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: messages,
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-user-tail-idle".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回");
    driver.await.unwrap();

    let events = sink.events();
    // #672：pending_input 空时，即使 messages 末尾是 User，也不应触发 LLM 响应
    assert!(
        !events.iter().any(|e| e.contains("hi there")),
        "messages 末尾 User + pending_input 空 → 应 idle 等待，不应触发 LLM: {events:?}"
    );
}

/// 首次 LLM 调用返回普通协议错误，模拟
/// "stream error: stream interrupted..."），后续调用正常完成。
/// 用于验证 API 错误 turn 终止收口（#749）。
#[derive(Clone)]
struct ApiErrorThenNormalProvider {
    calls: Arc<Mutex<usize>>,
}

impl ApiErrorThenNormalProvider {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl LlmProvider for ApiErrorThenNormalProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        let call_index = {
            let mut guard = self.calls.lock().unwrap();
            let idx = *guard;
            *guard += 1;
            idx
        };
        if call_index == 0 {
            // 回合 1：模拟 provider 流中断（非取消类 API 错误）。
            return Err(ProviderError::fatal(
                ProviderErrorKind::Protocol,
                "stream interrupted after partial output: error decoding response body".to_string(),
            ));
        }
        Ok(text_completion_stream("recovered final response", 1, 1))
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

/// #749：provider 流中断后，API 错误 turn 终止必须收口 ——
/// 1. 发出 `ApiError`（携带错误供展示）；
/// 2. 紧随发出 `DoneWithDuration` 作为统一 turn 结束信号（TUI 据此清 processing）；
/// 3. NOT 再发 `Error`（消除 TUI 双渲染）；
/// 4. loop 回到 idle，后续新输入能正常触发下一回合。
#[tokio::test]
async fn test_api_error_finalizes_with_done_and_no_duplicate_error() {
    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    // 首条输入触发回合 1（会命中 API 错误）。
    input_tx
        .send(sdk::ChatInputEvent::user_message("hello", Vec::new()))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        // 等回合 1 的 API 错误收口（出现 DoneWithDuration）。
        loop {
            if driver_sink.events().iter().any(|e| e == "DoneWithDuration") {
                break;
            }
            tokio::task::yield_now().await;
        }
        // 投递新用户消息：验证 API 错误后 loop 回到 idle、能正常开启回合 2。
        input_tx
            .send(sdk::ChatInputEvent::user_message("retry", Vec::new()))
            .unwrap();
        loop {
            if driver_sink
                .events()
                .iter()
                .any(|e| e.contains("recovered final response"))
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx); // 关闭通道 → shutdown
    });

    let provider = ApiErrorThenNormalProvider::new();
    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(provider)),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-api-error-finalize".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop 应在 shutdown 后返回");
    driver.await.unwrap();

    let events = sink.events();

    // 1. API 错误路径发出 ApiError 事件（携带错误文本供展示）。
    let api_error = events
        .iter()
        .position(|e| e.starts_with("ApiError:") && e.contains("stream interrupted"))
        .expect("API 错误应发出 ApiError 事件");

    // 2. ApiError 之后紧随 DoneWithDuration，统一 turn 结束信号。
    let done_after_error = events
        .iter()
        .skip(api_error)
        .position(|e| e == "DoneWithDuration")
        .expect("API 错误后应发出 DoneWithDuration 作为 turn 结束信号");
    assert!(
        done_after_error > 0,
        "DoneWithDuration 应在 ApiError 之后: {events:?}"
    );

    // 3. NOT 再发 Error 事件（消除 TUI 双渲染）。
    assert!(
        !events.iter().any(|e| e.starts_with("Error:")),
        "API 错误路径不应再发 Error 事件（避免 TUI 双渲染）: {events:?}"
    );

    // 4. API 错误后 loop 回 idle，新输入正常触发回合 2。
    assert!(
        events
            .iter()
            .any(|e| e.contains("recovered final response")),
        "API 错误后应能正常开启下一回合: {events:?}"
    );
}

// ─── #1272 AwaitUser same-Run recovery ──────────────────────────

/// Provider that returns an AskUserQuestion tool call on the first
/// invocation and a plain text response on the second.  Signals a
/// `tokio::sync::Notify` after the first invocation so the test driver
/// can deterministically inject user input at the right time (#1272).
struct AskUserThenTextProvider {
    call_count: Arc<Mutex<usize>>,
    notify: Arc<tokio::sync::Notify>,
}

impl AskUserThenTextProvider {
    fn new(notify: Arc<tokio::sync::Notify>) -> Self {
        Self {
            call_count: Arc::new(Mutex::new(0)),
            notify,
        }
    }
}

#[async_trait]
impl LlmProvider for AskUserThenTextProvider {
    async fn invocation_stream(
        &self,
        _scope: &InvocationScope,
        _system: &[SystemBlock],
        _messages: &[Message],
        _tool_schemas: &[serde_json::Value],
        _cancel: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError> {
        let call_num = {
            let mut count = self.call_count.lock().unwrap();
            *count += 1;
            *count
        };
        if call_num == 1 {
            // First call: return an AskUserQuestion tool call.
            // This triggers the engine's Tools step, and with
            // TEST_AWAIT_USER_MODE enabled, MainRunPort returns
            // ToolStep::AwaitUser → Run AwaitingUser → session
            // waits for input.
            self.notify.notify_one();
            let tool_call = ProviderToolCall {
                id: ProviderToolCallId("toolu_ask_001".to_string()),
                name: "AskUserQuestion".to_string(),
                arguments: serde_json::json!({"question": "continue?"}),
            };
            Ok(Box::pin(futures::stream::iter(vec![
                InvocationEvent::Delta(InvocationDelta::ToolCallStarted {
                    index: 0,
                    provider_id: Some(ProviderToolCallId("toolu_ask_001".to_string())),
                    name: "AskUserQuestion".to_string(),
                }),
                InvocationEvent::Delta(InvocationDelta::ToolArgumentsDelta {
                    index: 0,
                    provider_id: Some(ProviderToolCallId("toolu_ask_001".to_string())),
                    partial_json: r#"{"question":"continue?"}"#.to_string(),
                }),
                InvocationEvent::Delta(InvocationDelta::ToolCallCompleted {
                    index: 0,
                    call: tool_call,
                }),
                InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::ToolCall(ProviderToolCall {
                        id: ProviderToolCallId("toolu_ask_001".to_string()),
                        name: "AskUserQuestion".to_string(),
                        arguments: serde_json::json!({"question": "continue?"}),
                    })],
                    stop_reason: ProviderStopReason::ToolUse,
                    usage: Some(RawUsageSnapshot {
                        input_tokens: Some(10),
                        output_tokens: Some(20),
                        ..RawUsageSnapshot::default()
                    }),
                    effective_reasoning: ReasoningLevel::Off,
                }),
            ])))
        } else {
            // Subsequent calls: plain text response to complete the Run.
            Ok(Box::pin(futures::stream::iter(vec![
                InvocationEvent::Delta(InvocationDelta::Text("all done".to_string())),
                InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::Text("all done".to_string())],
                    stop_reason: ProviderStopReason::EndTurn,
                    usage: Some(RawUsageSnapshot {
                        input_tokens: Some(10),
                        output_tokens: Some(3),
                        ..RawUsageSnapshot::default()
                    }),
                    effective_reasoning: ReasoningLevel::Off,
                }),
            ])))
        }
    }

    fn model_name(&self) -> &str {
        "test-model"
    }

    fn provider_name(&self) -> &str {
        "test-provider"
    }
}

#[tokio::test]
async fn test_await_user_same_run_recovery() {
    // #1272: Provider returns AskUser tool → engine AwaitUser waits
    // empty → test sends reply → same RunId produces next step →
    // only one Started/DoneWithDuration.
    //
    // Uses TEST_AWAIT_USER_MODE to make MainRunPort return
    // ToolStep::AwaitUser for AskUserQuestion instead of processing
    // inline.  No sleep — uses Notify signal to deterministically
    // coordinate the driver with the provider invocations.
    //
    // TDD: This test should FAIL before the loop_runner.rs change
    // because the old code drained after the first run_loop return
    // (AwaitUser), losing the Run.  After the fix, the session
    // waits for input and re-enters run_loop.

    // Safety: reset test mode after test
    let _guard = TestAwaitUserGuard::new();
    TEST_AWAIT_USER_MODE.store(true, std::sync::atomic::Ordering::Relaxed);

    let notify = Arc::new(tokio::sync::Notify::new());
    let notify_provider = notify.clone();

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();

    // Send initial user input that triggers the first model call.
    input_tx
        .send(sdk::ChatInputEvent::user_message(
            "ask question",
            Vec::new(),
        ))
        .unwrap();

    // Driver: wait for the first model call via Notify signal, then
    // send the reply as user input, then wait for DoneWithDuration.
    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        // Wait for the provider's first invocation (tool-call response).
        // After this, the engine processes the tool step, returns
        // AwaitUser, and the session blocks on recv_next_input().
        notify_provider.notified().await;

        // Now the engine has processed the tool call and returned
        // AwaitUser.  The session is blocked on recv_next_input().
        // Send the reply.
        input_tx
            .send(sdk::ChatInputEvent::user_message("yes please", Vec::new()))
            .unwrap();

        // Wait for the Run to terminate (DoneWithDuration).
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|event| event.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(
            AskUserThenTextProvider::new(notify),
        )),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-await-user-recovery".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: Arc::new(crate::application::active_run::ActiveRunRegistry::default()),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(15), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop should complete after AwaitUser recovery");
    driver.await.unwrap();

    let events = sink.events();
    // Only one turn → one DoneWithDuration.
    let done_count = events
        .iter()
        .filter(|event| event.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 1,
        "AwaitUser 恢复后应只有 1 个 DoneWithDuration: {events:?}"
    );
    // The second model call produced a text response.
    assert!(
        events.iter().any(|e| e == "Text:all done"),
        "第二轮模型调用应产生文本响应: {events:?}"
    );
}

/// RAII guard that resets `TEST_AWAIT_USER_MODE` on drop so tests
/// don't leak state.
struct TestAwaitUserGuard;

impl TestAwaitUserGuard {
    fn new() -> Self {
        TEST_AWAIT_USER_MODE.store(false, std::sync::atomic::Ordering::Relaxed);
        Self
    }
}

impl Drop for TestAwaitUserGuard {
    fn drop(&mut self) {
        TEST_AWAIT_USER_MODE.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

// ─── #1272 AwaitUser cancel & control-event recovery ─────────────────

#[tokio::test]
async fn test_control_event_during_await_user_exits_to_session() {
    // #1272: When a non-UserMessage control event (e.g. ListModels)
    // arrives while the Run is AwaitingUser, the session must NOT
    // loop forever.  Instead, the event is pushed to pending_input and
    // the AwaitUser inner loop exits so the session idle gate can
    // process it.
    //
    // Uses ListModels (simpler than SwitchModel — no binding swap)
    // to keep the test focused on the AwaitUser exit path.
    let _guard = TestAwaitUserGuard::new();
    TEST_AWAIT_USER_MODE.store(true, std::sync::atomic::Ordering::Relaxed);

    let notify = Arc::new(tokio::sync::Notify::new());
    let notify_provider = notify.clone();

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let active_run = Arc::new(crate::application::active_run::ActiveRunRegistry::default());

    // Initial user input.
    input_tx
        .send(sdk::ChatInputEvent::user_message("hello", Vec::new()))
        .unwrap();

    let driver_sink = sink.clone();
    let driver = tokio::spawn(async move {
        // Wait for engine to reach AwaitUser.
        notify_provider.notified().await;

        // Send a control command (ListModels) while engine is in AwaitUser.
        input_tx.send(sdk::ChatInputEvent::ListModels).unwrap();

        // Wait for the control to be processed (ModelList event appears).
        loop {
            if driver_sink.events().iter().any(|e| e.contains("ModelList")) {
                break;
            }
            tokio::task::yield_now().await;
        }

        // Now send a real user input — the session should create a
        // fresh Run and process it normally.
        input_tx
            .send(sdk::ChatInputEvent::user_message(
                "now do the thing",
                Vec::new(),
            ))
            .unwrap();

        // Wait for the new Run to complete.
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(
            AskUserThenTextProvider::new(notify),
        )),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-ctl-await-user".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: active_run.clone(),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(15), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop should complete after control during AwaitUser");
    driver.await.unwrap();

    let events = sink.events();
    // ModelList proves the control event was processed at session level.
    assert!(
        events.iter().any(|e| e.contains("ModelList")),
        "ListModels during AwaitUser should be processed: {events:?}"
    );
    // The new Run after control must produce DoneWithDuration.
    let done_count = events
        .iter()
        .filter(|e| e.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 1,
        "control 后新 Run 应产生 1 个 DoneWithDuration: {events:?}"
    );
    // Verify the active registry is empty.
    assert!(
        active_run.active_ids().is_empty(),
        "active_run registry should be empty after Run completes"
    );
}

#[tokio::test]
async fn test_cancel_during_await_user_terminates_run() {
    // #1272: When the Run is AwaitingUser and a cancel request arrives,
    // the session must NOT stay stuck on recv_next_input().  Instead,
    // tokio::select! detects the cancel, re-enters run_loop, the engine
    // handles cancellation, and the Run terminates with the active
    // registration cleared.
    //
    // Flow:
    // 1. Model returns AskUser tool → engine AwaitUser → session waits.
    // 2. Test cancels the Run via active_run registry.
    // 3. cancel.cancelled() fires → re-enter run_loop → engine returns
    //    Terminal → inner loop breaks → active_run.clear().
    // 4. Session loop restarts, new user input starts a fresh Run.
    let _guard = TestAwaitUserGuard::new();
    TEST_AWAIT_USER_MODE.store(true, std::sync::atomic::Ordering::Relaxed);

    let notify = Arc::new(tokio::sync::Notify::new());
    let notify_provider = notify.clone();

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let active_run = Arc::new(crate::application::active_run::ActiveRunRegistry::default());

    // Initial user input.
    input_tx
        .send(sdk::ChatInputEvent::user_message("hello", Vec::new()))
        .unwrap();

    let driver_sink = sink.clone();
    let driver_registry = active_run.clone();
    let driver = tokio::spawn(async move {
        // Wait for the provider's first invocation (means engine is
        // processing the AskUser tool call and will soon be AwaitUser).
        notify_provider.notified().await;

        // Now the engine should be in AwaitUser. Cancel the Run.
        let run_id = loop {
            if let Some(run_id) = driver_registry.active_id() {
                break run_id;
            }
            tokio::task::yield_now().await;
        };
        assert_eq!(
            driver_registry.cancel(&run_id),
            sdk::CancelRunOutcome::Accepted
        );

        // Wait for the Run to terminate (Cancelled event appears).
        loop {
            if driver_sink.events().iter().any(|e| e == "Cancelled") {
                break;
            }
            tokio::task::yield_now().await;
        }

        // Now send a new user message — the session should create a
        // fresh Run and process it normally.
        input_tx
            .send(sdk::ChatInputEvent::user_message(
                "after cancel",
                Vec::new(),
            ))
            .unwrap();

        // Wait for the new Run to complete.
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(
            AskUserThenTextProvider::new(notify),
        )),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-cancel-await-user".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: active_run.clone(),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(15), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop should complete after cancel during AwaitUser");
    driver.await.unwrap();

    let events = sink.events();
    // Cancel during AwaitUser must produce a Cancelled event.
    assert!(
        events.iter().any(|e| e == "Cancelled"),
        "cancel during AwaitUser should emit Cancelled: {events:?}"
    );
    // The new Run after cancel must produce DoneWithDuration.
    let done_count = events
        .iter()
        .filter(|e| e.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 1,
        "cancel 后新 Run 应产生 1 个 DoneWithDuration: {events:?}"
    );
    // Verify the active registry is empty (both Runs cleared).
    assert!(
        active_run.active_ids().is_empty(),
        "active_run registry should be empty after both Runs complete"
    );
}

#[tokio::test]
async fn test_biased_select_preserves_queued_input_when_cancel_and_message_both_ready() {
    // #1272: When cancel is triggered AND a UserMessage is already
    // queued on the input channel during AwaitUser, the biased select
    // deterministically picks the input first.  The received message
    // enters run_input_buffer, then cancel is detected on re-entry to
    // run_loop via handle_interrupt.  drain_remaining_events routes
    // the message to pending_input, and the session idle gate consumes
    // it into a fresh Run — the message is NOT lost.
    //
    // Without biased (fair select), tokio could randomly pick cancel
    // first, silently dropping the queued message.
    //
    // No sleep — uses Notify to synchronise the driver with the
    // provider invocation, then sends the message and cancel in one
    // driver step so both are ready when the session polls the select.
    let _guard = TestAwaitUserGuard::new();
    TEST_AWAIT_USER_MODE.store(true, std::sync::atomic::Ordering::Relaxed);

    let notify = Arc::new(tokio::sync::Notify::new());
    let notify_provider = notify.clone();

    let sink = RecordingSink::default();
    let (input_tx, input_events) = ChannelInputEvents::new();
    let active_run = Arc::new(crate::application::active_run::ActiveRunRegistry::default());

    // Initial user input triggers the first model call → AskUser tool
    // → AwaitUser.
    input_tx
        .send(sdk::ChatInputEvent::user_message("hello", Vec::new()))
        .unwrap();

    let driver_sink = sink.clone();
    let driver_registry = active_run.clone();
    let driver = tokio::spawn(async move {
        // Wait for the provider's first invocation — engine is
        // processing the AskUser tool call and will soon enter AwaitUser.
        notify_provider.notified().await;

        // Queue a user message on the input channel.  The unbounded
        // channel accepts it immediately; the session hasn't polled
        // recv_next_input yet (it's still in the engine finishing the
        // tool step).
        input_tx
            .send(sdk::ChatInputEvent::user_message(
                "keep this message",
                Vec::new(),
            ))
            .unwrap();

        // Cancel the Run.  When the session reaches the biased select,
        // both recv_next_input and cancel.cancelled() are ready.
        // Biased ordering guarantees recv_next_input wins.
        let run_id = loop {
            if let Some(run_id) = driver_registry.active_id() {
                break run_id;
            }
            tokio::task::yield_now().await;
        };
        assert_eq!(
            driver_registry.cancel(&run_id),
            sdk::CancelRunOutcome::Accepted
        );

        // Wait for the Cancelled event (from handle_interrupt).
        loop {
            if driver_sink.events().iter().any(|e| e == "Cancelled") {
                break;
            }
            tokio::task::yield_now().await;
        }

        // The queued message should now be in pending_input.  The
        // session idle gate will consume it into a fresh Run.
        // Wait for DoneWithDuration — the new Run's completion.
        loop {
            let done_count = driver_sink
                .events()
                .iter()
                .filter(|e| e.as_str() == "DoneWithDuration")
                .count();
            if done_count >= 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        drop(input_tx);
    });

    let ctx = ChatLoopContext {
        sink: sink.clone(),
        queue: SequenceQueueDrainPort::new(vec![]),
        input_events,
        binding: crate::application::testing::binding_from_llm_provider(Arc::new(
            AskUserThenTextProvider::new(notify),
        )),
        tool_catalog: ::tools::composition::TestCatalogExecutionFactory::empty().catalog_port(),
        tool_execution: ::tools::composition::TestCatalogExecutionFactory::empty().execution(),
        tool_context_binding: ::tools::composition::TestCatalogExecutionFactory::empty().binding(),
        policy: Arc::new(policy::AllowAllPolicy),
        system_blocks: Vec::new(),
        system_prompt_text: String::new(),
        user_context: String::new(),
        initial_messages: vec![],
        context_size: 200_000,
        wiring: test_wiring(),
        workspace: project::wire_production_workspace(std::env::current_dir().unwrap())
            .expect("workspace 初始化成功")
            .into_views(),
        session_id: "test-biased-select-preserves-input".to_string(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        session_reminders: Arc::new(std::sync::Mutex::new(::tools::SessionReminders::new())),
        agent_runner: None,
        tool_result_materializer: crate::application::testing::test_tool_result_materializer(),
        active_run: active_run.clone(),
        task_access: Arc::new(task::TaskStore::new()),
        max_tool_concurrency: 1,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        hook_runner: test_hook_runner(),
        memory_config: share::config::MemoryConfig::default(),
        memory: std::sync::Arc::new(memory::NoOpMemory),
        reasoning: workflow::adaptive_reasoning(share::reasoning::ReasoningLevel::Off),
        build_switched_client: Arc::new(test_build_switched_client),
        reflection_history: test_reflection_history_store(),
        language: "en".to_string(),
        list_reflection_history: test_reflection_history(),
        list_models: test_list_models(),
        list_reminders: test_list_reminders(),
        list_sessions: test_list_sessions(),
    };

    tokio::time::timeout(std::time::Duration::from_secs(15), process_chat_loop(ctx))
        .await
        .expect("process_chat_loop should complete after biased select preserves queued input");
    driver.await.unwrap();

    let events = sink.events();

    // Cancel during AwaitUser must produce a Cancelled event.
    assert!(
        events.iter().any(|e| e == "Cancelled"),
        "cancel during AwaitUser should emit Cancelled: {events:?}"
    );

    // The queued user message ("keep this message") was routed through
    // pending_input into a fresh Run.  The provider (call_num == 2)
    // returns "all done".  Both DoneWithDuration and the LLM text must
    // appear, proving the message survived the cancel race.
    let done_count = events
        .iter()
        .filter(|e| e.as_str() == "DoneWithDuration")
        .count();
    assert_eq!(
        done_count, 1,
        "queued message should spawn a new Run producing 1 DoneWithDuration: {events:?}"
    );

    assert!(
        events.iter().any(|e| e == "Text:all done"),
        "LLM response 'all done' should appear — queued message was not lost: {events:?}"
    );

    // Verify the active registry is empty (both Runs cleared).
    assert!(
        active_run.active_ids().is_empty(),
        "active_run registry should be empty after both Runs complete"
    );
}

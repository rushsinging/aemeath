use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::application::context::coordination::ContextCoordinator;
use crate::application::loop_engine::chat::post_batch::run_post_tool_batch;
use crate::application::loop_engine::chat::reflection::{
    maybe_submit_pre_compact_reflection, should_run_turn_reflection, submit_interval_reflection,
};
use crate::application::loop_engine::chat::stream_handler::InvocationEventReducer;
use crate::application::loop_engine::chat::task_reminder::TaskReminderState;
use crate::application::loop_engine::chat::{
    ChatEventSink, InputEventDrainPort, QueueDrainPort, RuntimeStreamEvent, RuntimeTurnContext,
};
use crate::application::loop_engine::event_strategy::{EventStrategy, MainEventStrategy};
use crate::application::loop_engine::input_strategy::{BufferedInputAdapter, InputStrategy};
use crate::application::loop_engine::shared::compact_core;
use crate::application::loop_engine::{
    DrainEpoch, DrainOutcome, LoopEngineError, ModelStep, ToolGuardDecision,
};
use crate::application::run::context::RuntimeContext;
use crate::application::tool::agent::{Agent, ToolCall};
use crate::domain::agent_run::RunDomainEvent;
use crate::ports::{
    ContextRequest, ContextRequestId, Language as ContextLanguage, RunStepId, SessionId,
    SystemPromptSpec, TaskReminderSnapshot,
};

fn request_context_size(request: Option<&ContextRequest>) -> usize {
    request.map_or(1, |request| request.context_size.max(1))
}

pub(crate) fn request_log_context(
    parent: &logging::LogContext,
    model: &str,
    provider: &str,
    role: &str,
) -> logging::LogContext {
    parent.patched(logging::LogContextPatch {
        request_id: logging::FieldPatch::Set(uuid::Uuid::now_v7().to_string()),
        model: logging::FieldPatch::Set(model.to_string()),
        provider: logging::FieldPatch::Set(provider.to_string()),
        role: logging::FieldPatch::Set(role.to_string()),
        ..logging::LogContextPatch::default()
    })
}

#[cfg(test)]
fn loop_input_messages(inputs: &[crate::application::loop_engine::LoopInput]) -> Vec<Message> {
    inputs
        .iter()
        .map(|input| {
            if input.images.is_empty() {
                Message::user(input.text.clone())
            } else {
                super::super::input_gate::user_message_with_images(
                    input.text.clone(),
                    input.images.clone(),
                )
            }
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn fixture_bind_pending(
    pending: Vec<Message>,
    inputs: &[crate::application::loop_engine::LoopInput],
) -> (Vec<Message>, Vec<Message>) {
    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.replace_pending_step_messages(pending);
    let frozen = execution.freeze_step_input_messages(None, loop_input_messages(inputs));
    (frozen.clone(), frozen)
}

#[cfg(test)]
pub(crate) fn fixture_accepted_user_messages(
    pending: Vec<Message>,
    prefix: Option<Message>,
    inputs: &[crate::application::loop_engine::LoopInput],
) -> Vec<Message> {
    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.replace_pending_step_messages(pending);
    execution.freeze_step_input_messages(prefix, loop_input_messages(inputs));
    execution.accepted_input_snapshot()
}

#[cfg(test)]
pub(crate) fn fixture_finalize_messages(
    pending: Vec<Message>,
    produced: Vec<Message>,
) -> Vec<Message> {
    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.freeze_step_messages(pending.clone());
    for message in produced {
        execution.record_step_message(message);
    }
    pending
        .into_iter()
        .chain(execution.step_outcome())
        .collect()
}

/// Simulate a two-step freeze lifecycle (first step with buffer-sourced
/// inputs, second step with empty inputs as in an InternalContinuation).
/// Returns `(first_accepted, second_accepted)` — the accepted user
/// messages after each freeze.
///
/// Used by the idle-replay regression test to verify that gate-adopted
/// user input is not replayed when `pending` is empty (post-fix
/// contract).
#[cfg(test)]
pub(crate) fn fixture_two_step_accepted(
    pending: Vec<Message>,
    first_inputs: &[crate::application::loop_engine::LoopInput],
    second_inputs: &[crate::application::loop_engine::LoopInput],
) -> (Vec<Message>, Vec<Message>) {
    let mut execution = crate::application::run::execution_state::RunExecutionState::new();
    execution.replace_pending_step_messages(pending);
    execution.freeze_step_input_messages(None, loop_input_messages(first_inputs));
    let first_accepted = execution.accepted_input_snapshot();
    execution.freeze_step_input_messages(None, loop_input_messages(second_inputs));
    let second_accepted = execution.accepted_input_snapshot();
    (first_accepted, second_accepted)
}

/// #1385 Task 12: `S` generic eliminated — all event-sink access goes through
/// `RuntimeContext::event_sink()`.  BufferedInputAdapter now takes a
/// `ChatEventSinkHandle` directly.
#[allow(clippy::too_many_arguments)]
pub(crate) struct MainRunCapabilities<'a, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    // ── #1385: per-Run RuntimeContext — single source for all service contracts ──
    // Non-Option. Accessors below delegate to this for binding, tools, policy,
    // hooks, memory, reflection, reasoning, config, context, and interaction.
    // Event sink (`event_sink()`), usage tracker (`usage()`), and input buffer
    // (`input()`) also come from RuntimeContext — no duplicate fields.
    pub(crate) runtime_context: &'a RuntimeContext,

    pub(crate) queue: &'a Q,
    pub(crate) input_events: &'a I,
    pub(crate) system_prompt_text: &'a str,
    pub(crate) context_size: usize,
    pub(crate) workspace: &'a project::WorkspaceViews,
    pub(crate) session_id: &'a str,
    pub(crate) read_files: &'a Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub(crate) session_reminders: &'a Arc<std::sync::Mutex<tools::SessionReminders>>,
    pub(crate) agent_runner: &'a Option<Arc<dyn tools::AgentRunner>>,
    pub(crate) tool_result_materializer:
        &'a crate::application::tool::result_materialization::ToolResultMaterializer,
    pub(crate) max_tool_concurrency: usize,
    pub(crate) agent_semaphore: &'a Arc<tokio::sync::Semaphore>,
    pub(crate) reflection_tasks: &'a crate::application::reflection::ReflectionTaskAdapter,
    pub(crate) language: &'a str,
    /// Run-scoped input strategy: owns the buffer, continuation flags,
    /// pending-input reference, and drain/await logic
    /// (#1272 per-turn drain-or-seal linearization).
    pub(crate) input_strategy: BufferedInputAdapter<'a, Q, I>,
    pub(crate) run_id: sdk::RunId,
    pub(crate) active_run: &'a dyn crate::domain::agent_run::ActiveRunPort,
    pub(crate) turn_context: RuntimeTurnContext,
    // #1385 Task 12: last_total_tokens eliminated — usage tracker is the
    // single source via runtime_context.usage().
    pub(crate) task_reminder_state: &'a mut TaskReminderState,
    pub(crate) tool_identity:
        &'a crate::application::tool::coordination::identity::ToolIdentityRegistry,
    /// #1248 Task 5: Whether this Run is operating in plan mode.
    /// When true, Complete responses trigger plan approval before proceeding.
    pub(crate) plan_mode: bool,
}

impl<Q, I> MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    // ── #1385: Accessor methods that delegate to RuntimeContext ──
    // These replace the old bare fields (binding, tool_catalog, etc.) so
    // that all reads go through the single source of truth.

    #[inline]
    fn binding(&self) -> &Arc<crate::ports::ProviderBinding> {
        self.runtime_context.provider_ref()
    }
    #[inline]
    fn tool_catalog(&self) -> &Arc<dyn tools::ToolCatalogPort> {
        self.runtime_context.tool_catalog_ref()
    }
    #[inline]
    fn tool_execution(&self) -> &Arc<dyn tools::ToolExecutionPort> {
        self.runtime_context.tool_execution_ref()
    }
    #[inline]
    fn task_access(&self) -> &Arc<dyn task::TaskAccess> {
        self.runtime_context.task_ref()
    }
    #[inline]
    fn hook_runner(&self) -> &Arc<dyn hook::HookPort> {
        self.runtime_context.hooks_ref()
    }
    #[inline]
    fn memory(&self) -> &Arc<dyn memory::MemoryPort> {
        self.runtime_context.memory_ref()
    }
    #[inline]
    fn reflection_history(&self) -> &Arc<dyn memory::api::ReflectionHistoryStore> {
        self.runtime_context.reflection_history_ref()
    }
    #[inline]
    fn reasoning(&self) -> crate::ports::ReasoningLevel {
        *self
            .runtime_context
            .reasoning_ref()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
    #[inline]
    fn run_config(&self) -> &crate::application::run::config::RunConfigSnapshot {
        self.runtime_context.config_ref()
    }
    #[inline]
    fn memory_config(&self) -> &share::config::MemoryConfig {
        self.runtime_context.config_ref().config().memory()
    }
    /// Context coordinator, constructed lazily from RuntimeContext's ContextPort.
    #[inline]
    fn context_coordinator(&self) -> ContextCoordinator {
        ContextCoordinator::new(self.runtime_context.context())
    }

    /// Drain any remaining events from the Run-scoped buffer back to the
    /// session's `pending_input`. Called after `run_loop` returns so that
    /// unconsumed control events are not lost (#1272).
    ///
    /// #1272: When the buffer was sealed by `drain_or_seal`, UserMessage
    /// events should not be present (all admission paths now use
    /// `push_or_reject`). If any are found, they are logged and routed to
    /// `pending_input` explicitly rather than silently forwarded.
    pub(crate) fn drain_remaining_events(&mut self) {
        let sealed = self.input_strategy.run_input_buffer.is_sealed();
        let drained = self
            .input_strategy
            .run_input_buffer
            .with_lock(|b| b.drain_all());
        for event in drained {
            match &event {
                sdk::ChatInputEvent::UserMessage { .. } if sealed => {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "MainRunCapabilities: sealed buffer contained unconsumed UserMessage; routing to pending_input"
                    );
                    self.input_strategy.pending_input.push(event);
                }
                _ => self.input_strategy.pending_input.push(event),
            }
        }
    }

    fn freeze_request(
        &self,
        step_id: &RunStepId,
        pending_messages: Vec<Message>,
    ) -> ContextRequest {
        let task_reminder = self.task_access().reminder_snapshot();
        let task_reminder = task_reminder
            .current_batch
            .and_then(|batch_id| {
                self.task_access().batch_snapshot(batch_id).map(|snapshot| {
                    let stats = snapshot.stats();
                    TaskReminderSnapshot {
                        task_list_id: Some(snapshot.batch().id().to_string()),
                        summary: snapshot.batch().summary().map(str::to_owned),
                        pending: stats.pending,
                        in_progress: stats.in_progress,
                    }
                })
            })
            .unwrap_or_default();
        let raw_tool_schemas = self
            .tool_catalog()
            .snapshot(
                &tools::RegistryScopeName::new("main"),
                &tools::ToolProfileName::new("main-full"),
            )
            .map(|snapshot| snapshot.model_schemas())
            .unwrap_or_default();
        let tool_schemas = raw_tool_schemas
            .iter()
            .filter_map(|schema| {
                Some(crate::ports::ModelToolSchema {
                    name: schema.get("name")?.as_str()?.to_string(),
                    description: schema.get("description")?.as_str()?.to_string(),
                    input_schema: schema.get("input_schema")?.clone(),
                })
            })
            .collect::<Vec<_>>();
        ContextRequest {
            session_id: SessionId::new(self.session_id),
            request_id: ContextRequestId::new(uuid::Uuid::now_v7().to_string()),
            run_id: self.run_id.clone(),
            step_id: step_id.clone(),
            pending_messages,
            system_prompt: SystemPromptSpec::new(self.system_prompt_text),
            model_id: self.binding().model.model.clone(),
            effective_reasoning: self.reasoning(),
            task_reminder,
            language: ContextLanguage::new(self.language),
            agent_roles: std::collections::HashMap::new(),
            config_snapshot: self.run_config().config().clone(),
            context_size: self.context_size,
            max_output_tokens: self.binding().max_tokens as usize,
            last_api_total_tokens: self.runtime_context.usage().get(),
            tool_schemas,
            tool_schema_tokens: context::compact::estimate_tool_schemas_tokens(&raw_tool_schemas),
        }
    }

    /// 实时从 Project-owned `WorkspaceRead` 读取 `workspace_root`，避免 turn 内
    /// 切换 worktree 后使用过时路径。
    fn current_cwd(&self) -> PathBuf {
        self.workspace.read().current_workspace_root()
    }

    /// Unify UserMessage admission into the active Run's input buffer.
    /// Delegates to [`BufferedInputAdapter::admit_user_message`].
    async fn admit_user_message(&mut self, event: sdk::ChatInputEvent) {
        self.input_strategy.admit_user_message(event).await;
    }

    /// Route events received during a Run. User messages accumulate in the
    /// Run-scoped buffer and are consumed in-step within the same Run (#1272).
    /// Control events (commands, WithdrawAll) are handled immediately or
    /// forwarded to session `pending_input`.
    async fn queue_busy_event(&mut self, event: sdk::ChatInputEvent) {
        match event {
            sdk::ChatInputEvent::UserMessage { .. } => {
                self.admit_user_message(event).await;
            }
            sdk::ChatInputEvent::WithdrawAll => {
                let texts = self
                    .input_strategy
                    .run_input_buffer
                    .with_lock(|b| b.withdraw_all_user_texts());
                self.runtime_context
                    .event_sink()
                    .send_event(RuntimeStreamEvent::UserMessagesWithdrawn { texts })
                    .await;
            }
            // Commands are retained for the session idle gate. They never enter model context.
            other => self.input_strategy.pending_input.push(other),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn make_agent(
        tool_catalog: &Arc<dyn tools::ToolCatalogPort>,
        tool_execution: &Arc<dyn tools::ToolExecutionPort>,
        agent_runner: &Option<Arc<dyn tools::AgentRunner>>,
        memory: &Arc<dyn memory::MemoryPort>,
        language: &str,
        user_agent: &str,
        workspace: &project::WorkspaceViews,
        cancel: &CancellationToken,
        read_files: &Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
        session_reminders: &Arc<std::sync::Mutex<tools::SessionReminders>>,
        max_tool_concurrency: usize,
        agent_semaphore: &Arc<tokio::sync::Semaphore>,
        session_id: &str,
        run_id: &sdk::RunId,
    ) -> Agent {
        let catalog = tool_catalog
            .snapshot(
                &tools::RegistryScopeName::new("main"),
                &tools::ToolProfileName::new("main-full"),
            )
            .unwrap_or_else(|_| tools::ToolCatalogSnapshot::new("main", "main-full", Vec::new()));
        Agent {
            catalog,
            execution: tool_execution.clone(),
            ctx: tools::ToolExecutionContext::new(
                tools::ExecutionScope::builder(
                    run_id.to_string(),
                    workspace.read().workspace_id(),
                    workspace.read().current_workspace_root(),
                )
                .build(),
                tools::ToolExecutionPorts::new(
                    crate::adapters::tool_runtime::cancellation(cancel.clone()),
                    crate::application::workspace::access::RuntimeWorkspaceAccess::new(
                        workspace.clone(),
                    )
                    .read_access(),
                    Arc::new(tools::MutexReadSet(read_files.clone())),
                    Arc::new(tools::FixedPlanMode(None)),
                    memory.clone(),
                    Arc::new(tools::FixedGuidance {
                        language: language.to_string(),
                    }),
                )
                .with_user_agent(user_agent)
                .with_memory_context(
                    Some(session_id.to_string()),
                    Some(session_reminders.clone()),
                )
                .with_agent(agent_runner.clone()),
            ),
            max_tool_concurrency,
            agent_semaphore: agent_semaphore.clone(),
            workspace_persist: workspace.persist(),
            runtime_cancellation: cancel.clone(),
        }
    }

    async fn invoke_shared_model(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        cancel: &CancellationToken,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        crate::application::model::invocation::orchestrate_model_invocation(self, execution, cancel)
            .await
    }
}

// ── Main tool-round observation ──────────────────────────────────────────

struct MainToolRoundObserver {
    runtime_context: RuntimeContext,
    workspace_root: PathBuf,
}

#[async_trait]
impl crate::application::tool::coordination::ToolRoundObserver for MainToolRoundObserver {
    async fn results_materialized(
        &mut self,
        execution: &crate::application::run::execution_state::RunExecutionState,
        has_task_mutation: bool,
    ) {
        self.runtime_context
            .event_sink()
            .send_event(RuntimeStreamEvent::PostToolExecutionSync {
                messages: execution.messages_snapshot(),
            })
            .await;
        if has_task_mutation {
            let snapshot =
                crate::application::loop_engine::chat::task_snapshot::build_task_snapshot(
                    &**self.runtime_context.task_ref(),
                );
            self.runtime_context
                .event_sink()
                .send_event(RuntimeStreamEvent::TasksSnapshot {
                    tasks: Box::new(snapshot),
                })
                .await;
        }
    }

    async fn round_finished(&mut self, call_count: usize, turn: usize, cancel: &CancellationToken) {
        run_post_tool_batch(
            &self.runtime_context.event_sink(),
            self.runtime_context.hooks_ref(),
            cancel,
            call_count,
            turn,
            &self.workspace_root,
        )
        .await;
    }
}

// ── Model invocation lifecycle capability ──────────────────────────────

impl<Q, I> crate::application::model::invocation::ModelInvocationContext
    for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn runtime_context(&self) -> &RuntimeContext {
        self.runtime_context
    }

    fn role(&self) -> &str {
        "main"
    }

    fn request_log_context(&self, parent: &logging::LogContext) -> logging::LogContext {
        request_log_context(
            parent,
            self.binding().model.model.as_str(),
            self.binding().model.provider.as_str(),
            "default",
        )
    }

    fn context_size(
        &self,
        execution: &crate::application::run::execution_state::RunExecutionState,
    ) -> usize {
        request_context_size(execution.context_request())
    }

    fn committed_delta(&self) -> bool {
        true
    }

    fn build_reducer(
        &self,
    ) -> InvocationEventReducer<crate::application::loop_engine::chat::ChatEventSinkHandle> {
        InvocationEventReducer::with_tool_identity(
            self.runtime_context.event_sink(),
            self.tool_identity.clone(),
            self.turn_context.clone(),
        )
    }

    fn waiting_event_context(
        &self,
    ) -> Option<(
        crate::application::loop_engine::chat::ChatEventSinkHandle,
        RuntimeTurnContext,
    )> {
        Some((self.runtime_context.event_sink(), self.turn_context.clone()))
    }

    fn extract_tool_calls(
        &self,
        response: &crate::application::loop_engine::chat::InvocationResponse,
    ) -> Vec<ToolCall> {
        Agent::extract_tool_calls_with_ids(&response.assistant_message, |provider_id| {
            self.tool_identity.runtime_id_for_provider(provider_id)
        })
    }
}

#[async_trait]
impl<Q, I> crate::application::model::invocation::ModelInvocationLifecycle
    for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    async fn on_window(
        &mut self,
        execution: &crate::application::run::execution_state::RunExecutionState,
    ) {
        if let Some(window) = execution.context_window() {
            self.task_reminder_state
                .update_from_messages(execution.turn_count() as u64, &window.messages);
        }
    }

    async fn pump_while_invoking<T: Send>(
        &mut self,
        invocation: impl std::future::Future<Output = T> + Send,
    ) -> T {
        tokio::pin!(invocation);
        loop {
            tokio::select! {
                response = &mut invocation => break response,
                event = self.input_events.recv_next_input() => {
                    if let Some(event) = event {
                        self.queue_busy_event(event).await;
                    }
                }
            }
        }
    }

    async fn on_retry(&mut self, attempt: u32, delay: std::time::Duration) {
        self.runtime_context.event_sink().try_send_event(
            RuntimeStreamEvent::ModelInvocationRetrying {
                context: self.turn_context.clone(),
                attempt,
                delay,
            },
        );
    }

    async fn on_response(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        response: &crate::application::loop_engine::chat::InvocationResponse,
        elapsed_secs: f64,
    ) {
        if let Some(queued) = self.queue.drain_queued_input().await {
            for text in queued {
                self.queue_busy_event(sdk::ChatInputEvent::classify_text(text, Vec::new()))
                    .await;
            }
        }
        self.runtime_context
            .event_sink()
            .send_event(RuntimeStreamEvent::Usage {
                input: response.usage.input_tokens.unwrap_or(0),
                output: response.usage.output_tokens.unwrap_or(0),
                last_input: response.usage.input_tokens.unwrap_or(0),
                elapsed_secs,
            })
            .await;
        self.runtime_context
            .event_sink()
            .send_event(RuntimeStreamEvent::TurnStarted {
                messages: execution.messages_snapshot(),
            })
            .await;
    }

    async fn classify_terminal(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        response: &crate::application::loop_engine::chat::InvocationResponse,
        calls: Vec<ToolCall>,
        usage: crate::application::loop_engine::StepTokenUsage,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        if !calls.is_empty() {
            return Ok((
                ModelStep::Tools {
                    text: response.assistant_message.text_content(),
                    calls,
                },
                usage,
            ));
        }
        if should_run_turn_reflection(
            self.memory_config(),
            execution.turn_count(),
            false,
            &response.stop_reason,
            false,
        ) {
            let _ = submit_interval_reflection(
                self.reflection_tasks,
                self.memory_config(),
                execution.turn_count(),
                execution.messages(),
                self.binding(),
                self.system_prompt_text,
                self.language,
                self.memory(),
                self.reflection_history(),
            );
        }
        Ok((
            ModelStep::Complete {
                text: response.assistant_message.text_content(),
            },
            usage,
        ))
    }
}

#[async_trait]
impl<Q, I> crate::application::loop_engine::InputPort for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        self.input_strategy.drain_input(expected_epoch).await
    }

    fn schedule_internal_continuation(
        &mut self,
        kind: crate::application::loop_engine::InternalContinuationKind,
    ) {
        if matches!(
            kind,
            crate::application::loop_engine::InternalContinuationKind::ToolResults
        ) {
            self.input_strategy.pending_tool_results = true;
        }
    }

    async fn await_user_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        self.input_strategy.await_user_input(expected_epoch).await
    }
}

#[async_trait]
impl<Q, I> crate::application::loop_engine::EventSinkPort for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    async fn emit(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        events: Vec<RunDomainEvent>,
    ) -> Result<(), LoopEngineError> {
        let mut strategy = MainEventStrategy {
            sink: self.runtime_context.event_sink(),
            session_id: self.session_id,
            turn_context: &self.turn_context,
            task_access: self.task_access(),
            model: &self.binding().model.model,
            started_at: execution.started_at().unwrap_or_else(Instant::now),
            turn_count: execution.turn_count(),
            messages_snapshot: execution.messages_snapshot(),
        };
        strategy.emit(events).await
    }
}

#[async_trait]
impl<Q, I> crate::application::loop_engine::StepPersistencePort for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn take_step_input_prefix(&mut self) -> Option<Message> {
        self.input_strategy.pending_stop_hook_feedback.take()
    }

    fn build_context_request(
        &self,
        execution: &crate::application::run::execution_state::RunExecutionState,
        _run_id: &sdk::RunId,
        step_id: &RunStepId,
    ) -> Option<ContextRequest> {
        Some(self.freeze_request(step_id, execution.step_outcome()))
    }

    async fn accept_step_input(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        step_id: &RunStepId,
    ) -> Result<(), LoopEngineError> {
        let accepted = execution.accepted_input_snapshot();
        if accepted.is_empty() {
            return Ok(());
        }
        let request = execution
            .context_request()
            .ok_or_else(|| LoopEngineError::Adapter("ContextRequest 尚未冻结".to_string()))?;
        debug_assert_eq!(&request.step_id, step_id);
        self.context_coordinator()
            .append_accepted_input(request, accepted)
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        let adopted = execution.take_adopted_input();
        if !adopted.is_empty() {
            let queued = self
                .input_strategy
                .run_input_buffer
                .with_lock(|buffer| buffer.user_message_snapshot());
            self.runtime_context
                .event_sink()
                .send_event(RuntimeStreamEvent::UserMessagesAdopted {
                    items: adopted,
                    queued,
                })
                .await;
        }
        Ok(())
    }

    async fn persist_step_commit(
        &mut self,
        commit: &crate::application::loop_engine::StepCommit,
    ) -> Result<(), LoopEngineError> {
        let (Some(request), Some(expected_revision)) =
            (commit.request.as_ref(), commit.expected_revision)
        else {
            return Ok(());
        };
        self.context_coordinator()
            .append_finalized(
                request,
                commit.step_id.clone(),
                expected_revision,
                commit.cause,
                commit.messages.clone(),
                vec![],
                self.runtime_context.usage().get(),
            )
            .await
            .map(|_| ())
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))
    }
}

#[async_trait]
impl<Q, I> crate::application::loop_engine::CompactionPort for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    async fn needs_compaction(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
    ) -> Result<bool, LoopEngineError> {
        let (needed, window) =
            crate::application::loop_engine::shared::needs_compaction_with_window(
                execution.context_request(),
                &self.context_coordinator(),
            )
            .await?;
        *execution.context_window_mut() = Some(window);
        Ok(needed)
    }

    async fn compact(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        _cancel: &CancellationToken,
    ) -> Result<(), LoopEngineError> {
        // Freeze the pre-compact messages snapshot before invoking the context
        // adapter. Only the early window that compact will discard feeds the
        // PreCompact reflection; the recent tail stays in `recent_messages` and
        // is observable by the next LLM turn without going through Memory.
        let pre_compact_snapshot: Vec<share::message::Message> = execution
            .context_window()
            .map(|window| {
                context::compact::messages_selected_for_precompact_memory(&window.messages)
            })
            .unwrap_or_default();
        let source_revision = execution
            .context_window()
            .map(|window| window.backing_revision)
            .ok_or_else(|| LoopEngineError::Adapter("ContextWindow 尚未构建".to_string()))?;
        let request = execution.context_request().cloned();
        let outcome = compact_core(
            request.as_ref(),
            source_revision,
            &self.context_coordinator(),
            &self.runtime_context.usage(),
            execution.context_window_mut(),
        )
        .await?;
        // Production PreCompact reflection trigger (#1284): only the success
        // path submits the frozen pre-compact snapshot. Errors and `Skipped`
        // never enqueue a job. The submission is non-blocking and shares the
        // session-scoped slot with Interval and Manual triggers; the helper
        // returns `BusySkipped`/`DisabledSkipped` without blocking the caller.
        let _ = maybe_submit_pre_compact_reflection(
            &outcome,
            &pre_compact_snapshot,
            self.reflection_tasks,
            self.memory_config(),
            self.binding(),
            self.system_prompt_text,
            self.language,
            self.memory(),
            self.reflection_history(),
        );
        Ok(())
    }
}

#[async_trait]
impl<Q, I> crate::application::loop_engine::ModelInvocationPort for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    async fn invoke_model(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        cancel: &CancellationToken,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        self.invoke_shared_model(execution, cancel).await
    }
}

#[async_trait]
impl<Q, I> crate::application::loop_engine::StopHookPort for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn stop_hook_port(&self) -> std::sync::Arc<dyn hook::HookPort> {
        self.hook_runner().clone()
    }

    fn stop_hook_context(&self) -> crate::application::hook::stop_coordination::StopHookContext {
        crate::application::hook::stop_coordination::StopHookContext {
            turns: 0,
            workspace_root: self.current_cwd(),
            session_id: self.session_id.to_string(),
            language: self.language.to_string(),
        }
    }

    async fn begin_stop_hook_status(&mut self) -> Result<(), LoopEngineError> {
        use crate::application::loop_engine::chat::{
            RuntimeHookEvent, RuntimeHookEventStatus, RuntimeStreamEvent,
        };
        self.runtime_context
            .event_sink()
            .send_event(RuntimeStreamEvent::HookEvent(RuntimeHookEvent {
                hook_name: format!("{:?}", hook::HookPoint::Stop),
                status: RuntimeHookEventStatus::Running,
                matcher: None,
                command: None,
                result: None,
            }))
            .await;
        Ok(())
    }

    fn install_stop_hook_feedback(&mut self, message: share::message::Message) {
        self.input_strategy.stop_hook_feedback = Some(message);
    }

    async fn project_stop_hook_outcome(
        &mut self,
        execution: &crate::application::run::execution_state::RunExecutionState,
        outcome: &crate::application::hook::stop_coordination::StopHookOutcome,
    ) -> Result<(), LoopEngineError> {
        crate::application::loop_engine::chat::hook_ui::project_hook_dispatch(
            &self.runtime_context.event_sink(),
            outcome.point,
            &outcome.dispatch,
        )
        .await;
        if outcome.feedback_message.is_some() {
            self.runtime_context
                .event_sink()
                .send_event(RuntimeStreamEvent::StopHookBlocked {
                    messages: execution.messages_snapshot(),
                })
                .await;
        }
        Ok(())
    }
}

#[async_trait]
impl<Q, I> crate::application::loop_engine::ToolOrchestrationPort for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    async fn execute_tools(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        calls: &[(ToolCall, ToolGuardDecision)],
        cancel: &CancellationToken,
    ) -> Result<crate::application::tool::coordination::ToolRoundOutcome, LoopEngineError> {
        let workspace_root = self.current_cwd();
        let context = crate::application::tool::coordination::ToolRoundContext {
            runtime_context: self.runtime_context,
            agent: Self::make_agent(
                self.tool_catalog(),
                self.tool_execution(),
                self.agent_runner,
                self.memory(),
                self.language,
                self.run_config().config().user_agent(),
                self.workspace,
                cancel,
                self.read_files,
                self.session_reminders,
                self.max_tool_concurrency,
                self.agent_semaphore,
                self.session_id,
                &self.run_id,
            ),
            turn_context: self.turn_context.clone(),
            language: self.language,
            workspace_root: workspace_root.clone(),
            session_id: self.session_id,
            materializer: self.tool_result_materializer,
            log_patch: logging::LogContextPatch::default(),
        };
        let mut observer = MainToolRoundObserver {
            runtime_context: self.runtime_context.clone(),
            workspace_root,
        };
        crate::application::tool::coordination::orchestrate_tool_round(
            context,
            &mut observer,
            execution,
            run_id,
            step_id,
            calls,
            cancel,
        )
        .await
    }
}

#[async_trait]
impl<Q, I> crate::application::loop_engine::StuckHandlingPort for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    async fn on_stuck(
        &mut self,
        _execution: &crate::application::run::execution_state::RunExecutionState,
        decision: &crate::application::loop_engine::StuckDecision,
    ) -> Result<(), LoopEngineError> {
        let _ = decision;
        Ok(())
    }
}

impl<Q, I> crate::application::loop_engine::PlanApprovalPort for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn needs_plan_approval(&self) -> bool {
        self.plan_mode
    }
}

#[async_trait]
impl<Q, I> crate::application::loop_engine::InteractionMailboxPort for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn interaction_port(&self) -> &dyn crate::application::interaction::port::InteractionPort {
        self.runtime_context.interaction_ref().as_ref()
    }

    async fn publish_interaction(
        &mut self,
        _execution: &crate::application::run::execution_state::RunExecutionState,
        request: &sdk::InteractionRequest,
    ) -> Result<(), LoopEngineError> {
        self.runtime_context
            .event_sink()
            .send_event(RuntimeStreamEvent::InteractionRequested {
                request: request.clone(),
            })
            .await;
        Ok(())
    }

    fn set_pending_interaction_work(
        &mut self,
        execution: &mut crate::application::run::execution_state::RunExecutionState,
        work: crate::application::loop_engine::PendingInteractionWork,
    ) {
        execution.set_pending_interaction_work(work);
    }
}

impl<Q, I> crate::application::loop_engine::InteractionCompletionPort
    for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn interaction_execution_scope(&self) -> tools::ExecutionScope {
        let workspace = self.workspace.read();
        tools::ExecutionScope::builder(
            self.run_id.to_string(),
            workspace.workspace_id(),
            workspace.current_workspace_root(),
        )
        .build()
    }

    fn interaction_tool_execution(&self) -> &dyn tools::ToolExecutionPort {
        self.tool_execution().as_ref()
    }

    fn interaction_materializer(
        &self,
    ) -> &crate::application::tool::result_materialization::ToolResultMaterializer {
        self.tool_result_materializer
    }

    fn interaction_session_id(&self) -> &str {
        self.session_id
    }

    fn interaction_cancellation(
        &self,
        step_cancel: CancellationToken,
    ) -> std::sync::Arc<dyn tools::CancellationSignal> {
        std::sync::Arc::new(
            crate::application::run::context::RunCancellationScope::from_token(step_cancel),
        )
    }
}

impl<Q, I> crate::application::loop_engine::RunControlPort for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn take_control(&self, run_id: &sdk::RunId) -> Option<crate::domain::agent_run::RunControl> {
        debug_assert_eq!(run_id, &self.run_id);
        self.active_run.take_control(run_id)
    }
}

impl<Q, I> crate::application::loop_engine::RunLifecyclePort for MainRunCapabilities<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn claim_terminal(&self, run_id: &sdk::RunId) -> bool {
        self.active_run.claim_terminal(run_id)
    }

    fn claim_cancellation(&self, run_id: &sdk::RunId) -> bool {
        self.active_run.claim_cancellation(run_id)
    }

    fn register_step_scope(
        &self,
        run_id: &sdk::RunId,
        step_id: sdk::RunStepId,
        cancel: CancellationToken,
    ) {
        debug_assert_eq!(run_id, &self.run_id);
        self.active_run
            .set_main_active_step(run_id, step_id, cancel);
    }
}

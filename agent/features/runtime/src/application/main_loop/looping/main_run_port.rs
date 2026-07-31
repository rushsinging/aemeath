use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use share::message::{Message, MessageSource, Role};
use tokio_util::sync::CancellationToken;

use crate::application::context_coordination::ContextCoordinator;
use crate::application::loop_engine::event_strategy::{EventStrategy, MainEventStrategy};
use crate::application::loop_engine::input_strategy::{InputStrategy, MainInputStrategy};
use crate::application::loop_engine::llm_log::{log_llm_input, log_llm_output_and_tool_calls};
use crate::application::loop_engine::shared::compact_core;
use crate::application::loop_engine::tool_strategy::{self, ToolStrategy};
use crate::application::loop_engine::{
    DrainEpoch, DrainOutcome, LoopEngineError, LoopInput, ModelStep, RunLoopPort,
    ToolGuardDecision, ToolStep,
};
use crate::application::main_loop::looping::post_batch::run_post_tool_batch;
use crate::application::main_loop::looping::reflection::{
    maybe_submit_pre_compact_reflection, should_run_turn_reflection, submit_interval_reflection,
};
use crate::application::main_loop::looping::stream_handler::{
    should_emit_model_stream_waiting, InvocationEventReducer,
};
use crate::application::main_loop::looping::tools::{execute_tool_round, tool_results_for_api};
use crate::application::main_loop::looping::{
    ChatEventSink, InputEventDrainPort, QueueDrainPort, RuntimeStreamEvent, RuntimeTurnContext,
};
use crate::application::runtime_context::RuntimeContext;
use crate::application::subagent::{Agent, ToolCall};
use crate::domain::agent_run::{RunDomainEvent, ToolCallStatus};
use crate::ports::{
    ContextRequest, ContextRequestId, Language as ContextLanguage, RunStepId, SessionId,
    SystemPromptSpec,
};

/// Aborts a spawned request companion task even when the invocation future is dropped.
struct AbortTaskOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortTaskOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

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

/// 以语义所有权记录尚未绑定与当前 RunStep 已绑定的消息，禁止通过位置索引推断。
#[derive(Default)]
pub(crate) struct StepMessageOwnership {
    pending: Vec<Message>,
    active: Vec<Message>,
    accepted_input: Vec<Message>,
    outcome: Vec<Message>,
}

impl StepMessageOwnership {
    pub(crate) fn new(pending: Vec<Message>) -> Self {
        Self {
            pending,
            active: Vec::new(),
            accepted_input: Vec::new(),
            outcome: Vec::new(),
        }
    }

    fn freeze(&mut self, prefix: Option<Message>, inputs: &[LoopInput]) -> Vec<Message> {
        let mut messages = prefix.into_iter().collect::<Vec<_>>();
        if inputs.is_empty() {
            messages.extend(std::mem::take(&mut self.pending));
        } else {
            messages.extend(inputs.iter().map(|input| {
                if input.images.is_empty() {
                    Message::user(input.text.clone())
                } else {
                    super::super::input_gate::user_message_with_images(
                        input.text.clone(),
                        input.images.clone(),
                    )
                }
            }));
        }
        self.active = messages.clone();
        self.accepted_input = messages
            .iter()
            .filter(|message| {
                message.role == Role::User
                    && message.metadata.as_ref().is_none_or(|metadata| {
                        !matches!(
                            metadata.source,
                            MessageSource::SystemGenerated | MessageSource::StopHook
                        )
                    })
            })
            .cloned()
            .collect();
        self.outcome.clear();
        messages
    }

    fn accepted_user_messages(&self) -> Vec<Message> {
        self.accepted_input.clone()
    }

    fn record(&mut self, message: Message) {
        self.active.push(message.clone());
        self.outcome.push(message);
    }

    fn outcome(&self) -> Vec<Message> {
        self.outcome.clone()
    }

    fn committed(&mut self) {
        self.active.clear();
        self.accepted_input.clear();
        self.outcome.clear();
    }
}

#[cfg(test)]
pub(crate) fn fixture_bind_pending(
    pending: Vec<Message>,
    inputs: &[LoopInput],
) -> (Vec<Message>, Vec<Message>) {
    let mut ownership = StepMessageOwnership::new(pending);
    let frozen = ownership.freeze(None, inputs);
    (frozen.clone(), frozen)
}

#[cfg(test)]
pub(crate) fn fixture_accepted_user_messages(
    pending: Vec<Message>,
    prefix: Option<Message>,
    inputs: &[LoopInput],
) -> Vec<Message> {
    let mut ownership = StepMessageOwnership::new(pending);
    ownership.freeze(prefix, inputs);
    ownership.accepted_user_messages()
}

#[cfg(test)]
pub(crate) fn fixture_finalize_messages(
    pending: Vec<Message>,
    produced: Vec<Message>,
) -> Vec<Message> {
    let mut ownership = StepMessageOwnership::new(pending);
    let accepted = ownership.freeze(None, &[]);
    for message in produced {
        ownership.record(message);
    }
    accepted.into_iter().chain(ownership.outcome()).collect()
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
    first_inputs: &[LoopInput],
    second_inputs: &[LoopInput],
) -> (Vec<Message>, Vec<Message>) {
    let mut ownership = StepMessageOwnership::new(pending);
    ownership.freeze(None, first_inputs);
    let first_accepted = ownership.accepted_user_messages();
    ownership.freeze(None, second_inputs);
    let second_accepted = ownership.accepted_user_messages();
    (first_accepted, second_accepted)
}

/// #1385 Task 12: `S` generic eliminated — all event-sink access goes through
/// `RuntimeContext::event_sink()`.  MainInputStrategy now takes a
/// `ChatEventSinkHandle` directly.
#[allow(clippy::too_many_arguments)]
pub(crate) struct MainRunPort<'a, Q, I>
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
    pub(crate) context_request: Option<crate::ports::ContextRequest>,
    pub(crate) context_window: Option<crate::ports::ContextWindow>,
    /// 当前 RunStep 的显式消息所有权；历史长度不参与归属判断。
    pub(crate) step_messages: StepMessageOwnership,
    pub(crate) messages: Vec<Message>,
    pub(crate) context_size: usize,
    pub(crate) workspace: &'a project::WorkspaceViews,
    pub(crate) session_id: &'a str,
    pub(crate) read_files: &'a Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub(crate) session_reminders: &'a Arc<std::sync::Mutex<tools::SessionReminders>>,
    pub(crate) agent_runner: &'a Option<Arc<dyn tools::AgentRunner>>,
    pub(crate) tool_result_materializer:
        &'a crate::application::tool_result_materialization::ToolResultMaterializer,
    pub(crate) max_tool_concurrency: usize,
    pub(crate) agent_semaphore: &'a Arc<tokio::sync::Semaphore>,
    pub(crate) reflection_tasks: &'a crate::application::reflection::ReflectionTaskAdapter,
    pub(crate) language: &'a str,
    /// Run-scoped input strategy: owns the buffer, continuation flags,
    /// pending-input reference, and drain/await logic
    /// (#1272 per-turn drain-or-seal linearization).
    pub(crate) input_strategy: MainInputStrategy<'a, Q, I>,
    /// #1272: Per-turn adopted (InputId, Message) pairs collected during
    /// freeze_step from LoopInput::input_id. Emitted via UserMessagesAdopted
    /// after accept_step_input durable success. Cleared after emission.
    pub(crate) per_turn_adopted: Vec<(sdk::InputId, Message)>,
    pub(crate) run_id: sdk::RunId,
    pub(crate) active_run: &'a dyn crate::domain::agent_run::ActiveRunPort,
    pub(crate) turn_count: usize,
    pub(crate) turn_context: RuntimeTurnContext,
    // #1385 Task 12: last_total_tokens eliminated — usage tracker is the
    // single source via runtime_context.usage().
    pub(crate) tool_identity:
        &'a crate::application::tool_coordination::identity::ToolIdentityRegistry,
    pub(crate) started_at: Instant,
    /// #1248 Task 5: Whether this Run is operating in plan mode.
    /// When true, Complete responses trigger plan approval before proceeding.
    pub(crate) plan_mode: bool,
    // ── #1248 Task 5: Interaction state ──
    /// Stored interaction receivers keyed by request id.
    /// The engine calls `store_interaction` to deposit receivers with metadata,
    /// and `poll_interaction` to check for completions.
    pub(crate) interaction_receivers: Vec<(
        crate::application::interaction::InteractionRequestMetadata,
        tokio::sync::oneshot::Receiver<crate::application::interaction::InteractionCompletion>,
    )>,
    /// Completions consumed by the temporary AwaitingUser wakeup select and
    /// handed to the engine through its existing `poll_interaction` seam.
    pub(crate) resolved_interactions: Vec<crate::application::interaction::InteractionResolution>,
    /// #1248: Pending interaction work set by the engine for multi-interaction rounds.
    pub(crate) pending_work: Option<crate::application::loop_engine::PendingInteractionWork>,
}

impl<Q, I> MainRunPort<'_, Q, I>
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
    fn policy(&self) -> &dyn policy::PolicyPort {
        self.runtime_context.policy_ref().as_ref()
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
    fn run_config(&self) -> &crate::application::run_config::RunConfigSnapshot {
        self.runtime_context.config_ref()
    }
    #[inline]
    fn memory_config(&self) -> &share::config::MemoryConfig {
        self.runtime_context.config_ref().config().memory()
    }
    #[inline]
    fn cancel_token(&self) -> CancellationToken {
        self.runtime_context.cancel_ref().token().clone()
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
                        "MainRunPort: sealed buffer contained unconsumed UserMessage; routing to pending_input"
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
        let raw_tool_schemas = self
            .tool_catalog()
            .snapshot_for_run(
                &tools::RegistryScopeName::new("main"),
                &tools::ToolProfileName::new("main-full"),
                self.run_config().tool_selection(),
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

    async fn persist_step(
        &mut self,
        cause: crate::ports::FinalizeCause,
    ) -> Result<(), LoopEngineError> {
        let (Some(request), Some(window)) = (&self.context_request, &self.context_window) else {
            return Ok(());
        };
        let messages = self.step_messages.outcome();
        self.context_coordinator()
            .append_finalized(
                request,
                request.step_id.clone(),
                window.backing_revision,
                cause,
                Some(self.started_at.elapsed().as_millis() as u64),
                messages,
                vec![],
                self.runtime_context.usage().get(),
            )
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        self.step_messages.committed();
        Ok(())
    }

    /// Unify UserMessage admission into the active Run's input buffer.
    /// Delegates to [`MainInputStrategy::admit_user_message`].
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
        skill_extra_dirs: &[std::path::PathBuf],
        user_agent: &str,
        workspace: &project::WorkspaceViews,
        cancel: &CancellationToken,
        read_files: &Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
        session_reminders: &Arc<std::sync::Mutex<tools::SessionReminders>>,
        max_tool_concurrency: usize,
        agent_semaphore: &Arc<tokio::sync::Semaphore>,
        session_id: &str,
        context_port: Arc<dyn context::ports::ContextPort>,
        skill_load_state: Arc<dyn tools::SkillLoadStatePort>,
        run_id: &sdk::RunId,
        tool_selection: &share::config::ToolSelection,
    ) -> Agent {
        let catalog = tool_catalog
            .snapshot_for_run(
                &tools::RegistryScopeName::new("main"),
                &tools::ToolProfileName::new("main-full"),
                tool_selection,
            )
            .unwrap_or_else(|_| tools::ToolCatalogSnapshot::new("main", "main-full", Vec::new()));
        let available_tools = catalog
            .tools
            .iter()
            .map(|descriptor| descriptor.name.as_str().to_string())
            .collect();
        Agent {
            catalog: catalog.clone(),
            execution: tool_execution.clone(),
            context: Some(ContextCoordinator::new(context_port)),
            session_id: context::domain::SessionId::new(session_id),
            ctx: tools::ToolExecutionContext::new(
                tools::ExecutionScope::builder(
                    run_id.to_string(),
                    workspace.read().workspace_id(),
                    workspace.read().current_workspace_root(),
                )
                .build(),
                tools::ToolExecutionPorts::new(
                    crate::application::runtime_context::tool_cancellation_signal(cancel.clone()),
                    crate::application::workspace_access::RuntimeWorkspaceAccess::new(
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
                .with_skill_query(tools::SkillQuerySnapshot {
                    extra_dirs: skill_extra_dirs.to_vec(),
                    available_tools,
                })
                .with_catalog(Some(Arc::new(catalog.clone())))
                .with_user_agent(user_agent)
                .with_memory_context(
                    Some(session_id.to_string()),
                    Some(session_reminders.clone()),
                )
                .with_skill_load_state(tools::SkillLoadScope::main(), skill_load_state)
                .with_agent(agent_runner.clone())
                .with_selection(tool_selection.clone()),
            ),
            max_tool_concurrency,
            agent_semaphore: agent_semaphore.clone(),
            workspace_persist: workspace.persist(),
            runtime_cancellation: cancel.clone(),
        }
    }

    async fn invoke_model_impl(
        &mut self,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        if self.context_window.is_none() {
            if let Some(request) = &self.context_request {
                let window = self
                    .context_coordinator()
                    .build_window(request)
                    .await
                    .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
                self.context_window = Some(window);
            }
        }
        let window = self
            .context_window
            .clone()
            .ok_or_else(|| LoopEngineError::Adapter("ContextWindow 尚未构建".to_string()))?;
        let ctx =
            crate::application::loop_engine::llm_strategy::extract_invocation_context(&window);
        log_llm_input(
            &ctx.messages_for_api,
            window.messages.len(),
            &ctx.system_blocks,
            &ctx.tool_schemas,
            "main",
        );
        let requested_reasoning = {
            use crate::application::loop_engine::llm_strategy::LlmStrategy;
            self.reasoning_level()
        };

        let api_start = Instant::now();
        let mut coordinator =
            crate::application::model_invocation::ModelInvocationCoordinator::new();
        let resp = loop {
            let request_context = request_log_context(
                &logging::capture(),
                self.binding().model.model.as_str(),
                self.binding().model.provider.as_str(),
                "default",
            );
            let mut reducer = InvocationEventReducer::with_tool_identity(
                self.runtime_context.event_sink(),
                self.tool_identity.clone(),
                self.turn_context.clone(),
            );
            let response = logging::instrument(request_context.clone(), async {
                let progress_handle = reducer.progress_handle();
                let stream_cancel = self.cancel_token().clone();
                let provider = self.binding().provider.clone();
                let model = self.binding().model.clone();
                let max_tokens = self.binding().max_tokens;
                let request_tool_schemas = window.tool_schemas.clone();
                let messages_for_api = Arc::clone(&ctx.messages_for_api);
                let system_blocks = ctx.system_blocks.clone();
                let committed_delta = {
                    use crate::application::loop_engine::llm_strategy::LlmStrategy;
                    self.committed_delta()
                };
                let invocation_fut = async {
                    let mut request = crate::ports::InvocationRequest::new(
                        model,
                        messages_for_api,
                        crate::ports::InvocationOptions::new(max_tokens, requested_reasoning),
                    );
                    request.system = system_blocks;
                    request.tools = request_tool_schemas;
                    request.cancellation = stream_cancel.clone();
                    let stream = provider
                        .invoke(request, &stream_cancel)
                        .await
                        .map_err(|error| (error, false))?;
                    coordinator
                        .pull_stream(stream, &stream_cancel, committed_delta, |event| {
                            reducer.apply(event)
                        })
                        .await
                };
                let waiting_sink = self.runtime_context.event_sink();
                let waiting_context = self.turn_context.clone();
                let request_started_at = tokio::time::Instant::now();
                let waiting_task =
                    AbortTaskOnDrop(logging::spawn_instrumented(request_context, async move {
                        let mut next = request_started_at + Duration::from_secs(10);
                        let mut last_version = None;
                        loop {
                            tokio::time::sleep_until(next).await;
                            let snapshot = progress_handle
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner())
                                .snapshot();
                            if should_emit_model_stream_waiting(last_version, &snapshot) {
                                waiting_sink.try_send_event(
                                    RuntimeStreamEvent::ModelStreamWaiting {
                                        context: waiting_context.clone(),
                                        elapsed_secs: request_started_at.elapsed().as_secs(),
                                        phase: snapshot.phase.to_string(),
                                    },
                                );
                            }
                            last_version = Some(snapshot.visible_progress_version);
                            next += Duration::from_secs(10);
                        }
                    }));
                tokio::pin!(invocation_fut);
                let result = loop {
                    tokio::select! {
                        response = &mut invocation_fut => break response,
                        event = self.input_events.recv_next_input() => {
                            if let Some(event) = event {
                                self.queue_busy_event(event).await;
                            }
                        }
                    }
                };
                drop(waiting_task);
                result
            })
            .await;
            match response {
                Ok((response, _)) => break response,
                Err((error, _)) if error.is_cancelled() || self.cancel_token().is_cancelled() => {
                    return Err(LoopEngineError::Cancelled);
                }
                Err((error, visible_delta)) => {
                    let step = coordinator
                        .handle_failure(&error, visible_delta, &self.cancel_token())
                        .await;
                    crate::application::loop_engine::llm_strategy::map_retry_outcome(
                        step,
                        &error.to_string(),
                        self,
                    )
                    .await?;
                }
            }
        };
        let api_elapsed = api_start.elapsed().as_secs_f64();

        // Poll the non-blocking legacy queue at the model boundary. Busy user input is kept for a
        // fresh Run and never appended to this Run's model context.
        if let Some(queued) = self.queue.drain_queued_input().await {
            for text in queued {
                self.queue_busy_event(sdk::ChatInputEvent::classify_text(text, Vec::new()))
                    .await;
            }
        }

        let total_tokens = crate::application::token_usage::normalized_total_tokens(&resp.usage);
        // #1385 Task 12: Write to RuntimeContext's usage tracker — the single source.
        self.runtime_context.usage().update(total_tokens);

        let token_usage = crate::application::loop_engine::llm_strategy::build_step_token_usage(
            &resp,
            request_context_size(self.context_request.as_ref()) as u64,
            window.token_estimation.system_tokens,
            window.token_estimation.tool_schema_tokens,
            window.token_estimation.message_tokens,
        );

        self.runtime_context
            .event_sink()
            .send_event(RuntimeStreamEvent::Usage {
                input: resp.usage.input_tokens.unwrap_or(0),
                output: resp.usage.output_tokens.unwrap_or(0),
                last_input: resp.usage.input_tokens.unwrap_or(0),
                elapsed_secs: api_elapsed,
            })
            .await;
        self.messages.push(resp.assistant_message.clone());
        self.step_messages.record(resp.assistant_message.clone());
        self.runtime_context
            .event_sink()
            .send_event(RuntimeStreamEvent::TurnStarted {
                messages: self.messages.clone(),
            })
            .await;

        let calls = Agent::extract_tool_calls_with_ids(&resp.assistant_message, |provider_id| {
            self.tool_identity.runtime_id_for_provider(provider_id)
        });
        log_llm_output_and_tool_calls(
            self.binding().model.provider.as_str(),
            &resp,
            &calls,
            api_elapsed,
            "main",
        );
        if !calls.is_empty() {
            return Ok((
                ModelStep::Tools {
                    text: resp.assistant_message.text_content(),
                    calls,
                },
                token_usage,
            ));
        }

        // #1248 Task 7: TextOnly reasoning observation moved to shared
        // Loop engine — adapter no longer calls observe directly.
        if should_run_turn_reflection(
            self.memory_config(),
            self.turn_count,
            !calls.is_empty(),
            &resp.stop_reason,
            false,
        ) {
            let _ = submit_interval_reflection(
                self.reflection_tasks,
                self.memory_config(),
                self.turn_count,
                &self.messages,
                self.binding(),
                self.system_prompt_text,
                self.language,
                self.memory(),
                self.reflection_history(),
            );
        }

        // #1248 Task 6: Stop hook evaluation moved to shared Loop via
        // `evaluate_stop_hook` seam.  The adapter simply returns Complete
        // and the engine triggers the hook through RunLoopPort.
        Ok((
            ModelStep::Complete {
                text: resp.assistant_message.text_content(),
            },
            token_usage,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_tools_impl(
        &mut self,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        calls: &[(ToolCall, ToolGuardDecision)],
        cancel: &CancellationToken,
    ) -> Result<ToolStep, LoopEngineError> {
        if calls.is_empty() {
            return Ok(ToolStep::Continue);
        }
        let raw_calls: Vec<_> = calls.iter().map(|(call, _)| call.clone()).collect();
        let agent = Self::make_agent(
            self.tool_catalog(),
            self.tool_execution(),
            self.agent_runner,
            self.memory(),
            self.language,
            &self.run_config().config().skills().dirs,
            self.run_config().config().user_agent(),
            self.workspace,
            cancel,
            self.read_files,
            self.session_reminders,
            self.max_tool_concurrency,
            self.agent_semaphore,
            self.session_id,
            self.runtime_context.context(),
            self.runtime_context.skill_load_state(),
            &self.run_id,
            self.run_config().tool_selection(),
        );
        let sink = self.runtime_context.event_sink();
        let round_result = execute_tool_round(
            &self.turn_context,
            &raw_calls,
            self.tool_execution(),
            self.policy(),
            run_id,
            step_id,
            &agent,
            &sink,
            self.hook_runner(),
            cancel,
            self.language,
            &self.current_cwd(),
            calls,
        )
        .await;

        let has_interaction =
            !round_result.suspensions.is_empty() || !round_result.approvals.is_empty();

        if has_interaction {
            // #1248: Collect interaction call IDs so we can separate them from
            // completed non-interaction results.
            let interaction_call_ids: std::collections::HashSet<sdk::ToolCallId> = round_result
                .suspensions
                .iter()
                .map(|s| s.call.id.clone())
                .chain(round_result.approvals.iter().map(|a| a.call.id.clone()))
                .collect();

            // Process non-interaction results: record messages, events, reasoning.
            let non_interaction_results: Vec<_> = round_result
                .results
                .iter()
                .filter(|r| !interaction_call_ids.contains(&r.call_id))
                .cloned()
                .collect();
            if !non_interaction_results.is_empty() {
                // Reasoning graph does not observe tool completion; tool results
                // only advance the shared Run state machine.
                let has_task_mutation = non_interaction_results.iter().any(|result| {
                    crate::application::main_loop::looping::events::is_task_store_mutation(
                        &result.tool_name,
                    )
                });
                let tool_results = tool_results_for_api(
                    self.tool_result_materializer,
                    non_interaction_results,
                    self.session_id,
                )
                .await;
                self.messages.push(tool_results.clone());
                self.step_messages.record(tool_results);
                self.runtime_context
                    .event_sink()
                    .send_event(RuntimeStreamEvent::PostToolExecutionSync {
                        messages: self.messages.clone(),
                    })
                    .await;
                if has_task_mutation {
                    let snapshot =
                        crate::application::main_loop::looping::task_snapshot::build_task_snapshot(
                            &**self.task_access(),
                        );
                    self.runtime_context
                        .event_sink()
                        .send_event(RuntimeStreamEvent::TasksSnapshot {
                            tasks: Box::new(snapshot),
                        })
                        .await;
                }
            }

            // Build completed_results from guarded_calls minus interaction calls.
            let completed_results: Vec<(sdk::ToolCallId, ToolCallStatus)> = calls
                .iter()
                .filter(|(call, _)| !interaction_call_ids.contains(&call.id))
                .map(|(call, decision)| {
                    let bypassed = round_result.fuse_bypassed.contains(&call.id);
                    let status = if matches!(decision, ToolGuardDecision::Allow) || bypassed {
                        ToolCallStatus::Success
                    } else {
                        ToolCallStatus::Cancelled
                    };
                    (call.id.clone(), status)
                })
                .collect();

            let fuse_bypassed = round_result.fuse_bypassed.clone();

            if !round_result.suspensions.is_empty() {
                return Ok(ToolStep::InteractionSuspended {
                    suspended: round_result.suspensions,
                    completed_results,
                    fuse_bypassed,
                });
            }
            if !round_result.approvals.is_empty() {
                return Ok(ToolStep::AwaitingToolApproval {
                    calls_needing_approval: round_result.approvals,
                    completed_results,
                    fuse_bypassed,
                });
            }
            // Fallthrough: should not happen, but continue as normal
        }

        let cancelled = cancel.is_cancelled();
        let all_results = if cancelled {
            crate::application::tool_coordination::complete_cancelled_tool_round(
                &raw_calls,
                round_result.results,
            )
        } else {
            round_result.results
        };

        // Reasoning graph does not observe tool completion.
        let has_task_mutation = all_results.iter().any(|result| {
            crate::application::main_loop::looping::events::is_task_store_mutation(
                &result.tool_name,
            )
        });
        let tool_results =
            tool_results_for_api(self.tool_result_materializer, all_results, self.session_id).await;
        self.messages.push(tool_results.clone());
        self.step_messages.record(tool_results);
        self.runtime_context
            .event_sink()
            .send_event(RuntimeStreamEvent::PostToolExecutionSync {
                messages: self.messages.clone(),
            })
            .await;
        if has_task_mutation {
            let snapshot =
                crate::application::main_loop::looping::task_snapshot::build_task_snapshot(
                    &**self.task_access(),
                );
            self.runtime_context
                .event_sink()
                .send_event(RuntimeStreamEvent::TasksSnapshot {
                    tasks: Box::new(snapshot),
                })
                .await;
        }
        if cancelled {
            return Err(LoopEngineError::Cancelled);
        }
        run_post_tool_batch(
            &sink,
            self.hook_runner(),
            &agent.runtime_cancellation,
            raw_calls.len(),
            self.turn_count,
            &self.current_cwd(),
        )
        .await;
        // #1272: tool results are an explicit InternalContinuation, not a
        // fresh batch of user input. Mark the port so the next drain will
        // emit `InternalContinuation::ToolResults` instead of an empty Ready.
        self.mark_tool_results_pending();
        Ok(tool_strategy::step_from_fuse_bypass(
            round_result.fuse_bypassed,
        ))
    }
}

// ── ToolStrategy impl ─────────────────────────────────────────────────

impl<Q, I> ToolStrategy for MainRunPort<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn mark_tool_results_pending(&mut self) {
        self.input_strategy.pending_tool_results = true;
    }
}

// ── LlmStrategy impl ─────────────────────────────────────────────────

#[async_trait]
impl<Q, I> crate::application::loop_engine::llm_strategy::LlmStrategy for MainRunPort<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn reasoning_level(&self) -> crate::ports::ReasoningLevel {
        self.reasoning()
    }

    fn committed_delta(&self) -> bool {
        true
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
}

#[async_trait]
impl<Q, I> RunLoopPort for MainRunPort<'_, Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn freeze_step(&mut self, step_id: &RunStepId, inputs: &[LoopInput]) {
        // #1272: consume from pending_stop_hook_feedback — drain_input took
        // from stop_hook_feedback and relayed here so freeze_step can
        // inject the feedback as a system-prefix message.
        let feedback = self.input_strategy.pending_stop_hook_feedback.take();
        let has_stop_hook_feedback = feedback.is_some();
        let _pending_messages = self.step_messages.freeze(feedback, inputs);
        if has_stop_hook_feedback {
            self.messages
                .extend(inputs.iter().map(|input| Message::user(input.text.clone())));
        }
        // #1272 per-turn drain identity: receipts are captured by RunInputBuffer while it still
        // owns the typed ChatInputEvent. This keeps model-only Skill prompts out of TUI JSON.
        self.per_turn_adopted = self
            .input_strategy
            .run_input_buffer
            .with_lock(|buffer| buffer.take_drained_adopted());
        if !self.per_turn_adopted.is_empty() {
            let input_ids: Vec<_> = self
                .per_turn_adopted
                .iter()
                .map(|(id, _)| id.as_str().to_string())
                .collect();
            log::debug!(
                target: crate::LOG_TARGET,
                "[loop_debug] freeze_step run_id={} step_id={} input_ids={:?} count={}",
                self.run_id,
                step_id,
                input_ids,
                self.per_turn_adopted.len()
            );
        }
        self.context_request = Some(self.freeze_request(step_id, self.step_messages.outcome()));
        self.context_window = None;
    }

    async fn accept_step_input(&mut self, step_id: &RunStepId) -> Result<(), LoopEngineError> {
        let accepted = self.step_messages.accepted_user_messages();
        if accepted.is_empty() {
            return Ok(());
        }
        let request = self
            .context_request
            .as_ref()
            .ok_or_else(|| LoopEngineError::Adapter("ContextRequest 尚未冻结".to_string()))?;
        debug_assert_eq!(&request.step_id, step_id);
        self.context_coordinator()
            .append_accepted_input(request, accepted)
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;

        // #1272 per-turn drain identity: emit UserMessagesAdopted strictly
        // after durable accept succeeds. The TUI uses this to clear queued
        // placeholders by input_id and append formal user messages.
        let adopted = std::mem::take(&mut self.per_turn_adopted);
        if !adopted.is_empty() {
            let queued = self
                .input_strategy
                .run_input_buffer
                .with_lock(|b| b.user_message_snapshot());
            let input_ids: Vec<_> = adopted
                .iter()
                .map(|(id, _)| id.as_str().to_string())
                .collect();
            let queued_ids: Vec<_> = queued
                .iter()
                .map(|(id, _)| id.as_str().to_string())
                .collect();
            log::debug!(
                target: crate::LOG_TARGET,
                "[loop_debug] accept_step_input emitting UserMessagesAdopted run_id={} step_id={} adopt_ids={:?} adopt_count={} queued_ids={:?} queued_count={}",
                self.run_id,
                step_id,
                input_ids,
                adopted.len(),
                queued_ids,
                queued.len(),
            );
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

    /// #1272: Delegates to [`MainInputStrategy::drain_input`].
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        self.input_strategy.drain_input(expected_epoch).await
    }

    /// #1280: Delegates to [`MainInputStrategy::await_user_input`].
    /// Cancellation / timeout is handled by the engine's `await_interruptible`.
    async fn await_user_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        self.input_strategy.await_user_input(expected_epoch).await
    }

    /// #1425 temporary compatibility wakeup.
    ///
    /// Main previously parked only on `input_events`, so an accepted interaction
    /// reply could not wake the Run until unrelated user input arrived. Keep this
    /// select until the concurrent RuntimeContext/Interaction refactor provides
    /// one owner-level AwaitingUser wakeup seam. That refactor MUST preserve the
    /// invariant that a reply alone resumes the Run, without an extra ChatInputEvent.
    async fn await_user_wakeup(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        if self.interaction_receivers.is_empty() {
            return self.input_strategy.await_user_input(expected_epoch).await;
        }

        tokio::select! {
            biased;
            resolution = async {
                let (metadata, receiver) = self
                    .interaction_receivers
                    .first_mut()
                    .expect("checked non-empty above");
                match receiver.await {
                    Ok(completion) => {
                        crate::application::interaction::InteractionResolution::Resolved {
                            metadata: metadata.clone(),
                            completion,
                        }
                    }
                    Err(_) => crate::application::interaction::InteractionResolution::Closed {
                        metadata: metadata.clone(),
                    },
                }
            } => {
                self.interaction_receivers.remove(0);
                self.resolved_interactions.push(resolution);
                Ok(DrainOutcome::NoInput { epoch: expected_epoch })
            }
            outcome = self.input_strategy.await_user_input(expected_epoch) => outcome,
        }
    }

    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
        let (needed, window) =
            crate::application::loop_engine::shared::needs_compaction_with_window(
                self.context_request.as_ref(),
                &self.context_coordinator(),
            )
            .await?;
        self.context_window = Some(window);
        Ok(needed)
    }

    async fn compact(&mut self, _cancel: &CancellationToken) -> Result<(), LoopEngineError> {
        // Freeze the pre-compact messages snapshot before invoking the context
        // adapter. Only the early window that compact will discard feeds the
        // PreCompact reflection; the recent tail stays in `recent_messages` and
        // is observable by the next LLM turn without going through Memory.
        let pre_compact_snapshot: Vec<share::message::Message> = self
            .context_window
            .as_ref()
            .map(|window| {
                context::compact::messages_selected_for_precompact_memory(&window.messages.to_vec())
            })
            .unwrap_or_default();
        let source_revision = self
            .context_window
            .as_ref()
            .map(|window| window.backing_revision)
            .ok_or_else(|| LoopEngineError::Adapter("ContextWindow 尚未构建".to_string()))?;
        let outcome = compact_core(
            self.context_request.as_ref(),
            source_revision,
            &self.context_coordinator(),
            &self.runtime_context.usage(),
            &mut self.context_window,
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

    async fn invoke_model(
        &mut self,
        _cancel: &CancellationToken,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        self.invoke_model_impl().await
    }

    /// #1248 Task 6: Shared Loop evaluates Stop hook via this seam.
    /// Uses the existing `dispatch_hook` to emit HookEvent UI events,
    /// then interprets the returned `RuntimeHookDispatch` as a typed
    /// `StopHookDecision`.
    async fn evaluate_stop_hook(
        &mut self,
        turns: usize,
    ) -> Result<crate::application::stop_hook_coordination::StopHookDecision, LoopEngineError> {
        use crate::application::hook_types::RuntimeHookDirective;
        use crate::application::main_loop::looping::hook_ui::dispatch_hook;
        use crate::application::stop_hook_coordination::StopHookDecision;

        let sink = self.runtime_context.event_sink();
        let dispatch = dispatch_hook(
            self.hook_runner(),
            &sink,
            hook::HookInvocation::Stop(hook::StopInput { turns }),
            &self.current_cwd(),
            &self.cancel_token(),
        )
        .await;

        log::info!(
            target: crate::LOG_TARGET,
            "[stop_hook] directive={:?} executions={}",
            dispatch.directive,
            dispatch.executions.len(),
        );

        match &dispatch.directive {
            RuntimeHookDirective::Block { reason } => {
                let detail = dispatch
                    .block_detail
                    .clone()
                    .expect("Stop hook Block must carry the blocking subscription detail");

                let feedback_msg =
                    crate::application::stop_hook_coordination::materialize_stop_hook_feedback(
                        &detail,
                        reason,
                        self.session_id,
                        self.language,
                    )
                    .await;

                let llm_text = format!(
                    "<system-reminder>\n{}\n</system-reminder>",
                    feedback_msg.llm_text
                );
                let payload = feedback_msg.payload.clone();
                let msg =
                    share::message::Message::stop_hook_feedback(llm_text, feedback_msg.payload);
                self.input_strategy.stop_hook_feedback = Some(msg.clone());
                self.step_messages.record(msg.clone());
                self.messages.push(msg);

                // Emit UI event.
                self.runtime_context
                    .event_sink()
                    .send_event(RuntimeStreamEvent::StopHookBlocked {
                        messages: self.messages.clone(),
                    })
                    .await;

                use crate::application::stop_hook_coordination::StopHookBlock;

                Ok(StopHookDecision::Block(Box::new(StopHookBlock {
                    reason: reason.clone(),
                    detail,
                    messages: dispatch.messages.clone(),
                    feedback:
                        crate::application::stop_hook_coordination::StopHookFeedbackMaterial {
                            llm_text: feedback_msg.llm_text,
                            payload,
                        },
                })))
            }
            _ => Ok(StopHookDecision::Proceed),
        }
    }

    async fn finalize_step(&mut self, step_id: &RunStepId) -> Result<(), LoopEngineError> {
        let Some(request) = &self.context_request else {
            return Ok(());
        };
        debug_assert_eq!(&request.step_id, step_id);
        self.persist_step(crate::ports::FinalizeCause::Completed)
            .await
    }

    async fn finalize_cancelled_step(
        &mut self,
        step_id: &RunStepId,
    ) -> Result<(), LoopEngineError> {
        let Some(request) = &self.context_request else {
            return Ok(());
        };
        debug_assert_eq!(&request.step_id, step_id);
        self.persist_step(crate::ports::FinalizeCause::UserCancelledStep)
            .await
    }

    async fn execute_tools(
        &mut self,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        calls: &[(ToolCall, ToolGuardDecision)],
        cancel: &CancellationToken,
    ) -> Result<ToolStep, LoopEngineError> {
        self.execute_tools_impl(run_id, step_id, calls, cancel)
            .await
    }

    // ── #1248 Task 5: Interaction seams ──

    fn interaction_port(&self) -> &dyn crate::application::interaction::InteractionPort {
        self.runtime_context.interaction_ref().as_ref()
    }

    async fn publish_interaction(
        &mut self,
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

    fn store_interaction(
        &mut self,
        metadata: crate::application::interaction::InteractionRequestMetadata,
        receiver: tokio::sync::oneshot::Receiver<
            crate::application::interaction::InteractionCompletion,
        >,
    ) -> Result<(), LoopEngineError> {
        self.interaction_receivers.push((metadata, receiver));
        Ok(())
    }

    async fn poll_interaction(
        &mut self,
    ) -> Result<Option<crate::application::interaction::InteractionResolution>, LoopEngineError>
    {
        if !self.resolved_interactions.is_empty() {
            return Ok(Some(self.resolved_interactions.remove(0)));
        }
        // Check each stored receiver; return the first completed one.
        let mut resolved = None;
        let mut remaining = Vec::new();
        for (metadata, mut rx) in std::mem::take(&mut self.interaction_receivers) {
            match rx.try_recv() {
                Ok(completion) => {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[MainRunPort::poll_interaction] resolved rid={:?}",
                        metadata.request_id
                    );
                    resolved = Some(
                        crate::application::interaction::InteractionResolution::Resolved {
                            metadata,
                            completion,
                        },
                    );
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "[MainRunPort::poll_interaction] closed rid={:?}",
                        metadata.request_id
                    );
                    resolved = Some(
                        crate::application::interaction::InteractionResolution::Closed { metadata },
                    );
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    remaining.push((metadata, rx)); // still pending
                }
            }
        }
        self.interaction_receivers = remaining;
        Ok(resolved)
    }

    fn set_pending_interaction_work(
        &mut self,
        work: crate::application::loop_engine::PendingInteractionWork,
    ) {
        self.pending_work = Some(work);
    }

    async fn finish_interaction_work(
        &mut self,
        metadata: &crate::application::interaction::InteractionRequestMetadata,
        completion: &crate::application::interaction::InteractionCompletion,
        _cancel: &CancellationToken,
    ) -> Result<crate::application::loop_engine::InteractionWorkOutcome, LoopEngineError> {
        use crate::application::interaction::InteractionCompletion;
        use crate::application::subagent::ToolExecution;
        use crate::domain::agent_run::InteractionContinuation;

        // Take the current work: current item is what's being resolved,
        // queue is what remains to be started.
        let work = self.pending_work.take();
        let current = work.as_ref().and_then(|w| w.current.clone());
        let remaining_queue: Vec<_> = work.as_ref().map(|w| w.queue.clone()).unwrap_or_default();

        let (call_id, status) = match (&metadata.continuation, completion) {
            (
                InteractionContinuation::CompleteToolCall(id),
                InteractionCompletion::Replied(reply),
            ) => {
                // UserQuestions: serialize answers as tool result.
                // Use the original call's provider_id and name from the
                // suspended_call stored in the current item.
                let (provider_id, tool_name) = current
                    .as_ref()
                    .and_then(|ci| ci.suspended_call.as_ref())
                    .map(|sc| (sc.call.provider_id.clone(), sc.call.name.clone()))
                    .unwrap_or_else(|| (String::new(), "AskUserQuestion".to_string()));
                if let sdk::InteractionReply::UserQuestions(answers) = reply {
                    let text = answers
                        .iter()
                        .enumerate()
                        .map(|(i, a)| format!("Q{}: {}", i + 1, a.0))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let outcome = tools::ToolOutcome {
                        text,
                        data: serde_json::json!({"status": "ok", "answers": answers}),
                        is_error: false,
                        images: Vec::new(),
                    };
                    let execution =
                        ToolExecution::from_parts(id.clone(), provider_id, tool_name, outcome);
                    // Append tool result message and send event
                    let materializer = self.tool_result_materializer;
                    let msg = crate::application::main_loop::looping::tools::tool_results_for_api(
                        materializer,
                        vec![execution],
                        self.session_id,
                    )
                    .await;
                    self.messages.push(msg.clone());
                    self.step_messages.record(msg);
                    self.mark_tool_results_pending();
                    (id.clone(), ToolCallStatus::Success)
                } else {
                    (id.clone(), ToolCallStatus::Error)
                }
            }
            (
                InteractionContinuation::CompleteToolCall(id),
                InteractionCompletion::Cancelled(_),
            ) => {
                // User questions cancelled: write error result using original call info
                let (provider_id, tool_name) = current
                    .as_ref()
                    .and_then(|ci| ci.suspended_call.as_ref())
                    .map(|sc| (sc.call.provider_id.clone(), sc.call.name.clone()))
                    .unwrap_or_else(|| (String::new(), "AskUserQuestion".to_string()));
                let outcome = tools::ToolOutcome::error("user cancelled interaction");
                let execution =
                    ToolExecution::from_parts(id.clone(), provider_id, tool_name, outcome);
                let materializer = self.tool_result_materializer;
                let msg = crate::application::main_loop::looping::tools::tool_results_for_api(
                    materializer,
                    vec![execution],
                    self.session_id,
                )
                .await;
                self.messages.push(msg.clone());
                self.step_messages.record(msg);
                (id.clone(), ToolCallStatus::Cancelled)
            }
            (
                InteractionContinuation::ContinueToolApproval(id),
                InteractionCompletion::Replied(reply),
            ) => {
                if matches!(
                    reply,
                    sdk::InteractionReply::ToolApproval(sdk::ApprovalDecision::Approve)
                ) {
                    // Execute the approved tool directly using the approval_call
                    // from the current item — no re-policy evaluation.
                    if let Some(approval_call) =
                        current.as_ref().and_then(|ci| ci.approval_call.clone())
                    {
                        let call = &approval_call.call;
                        let ws_read = self.workspace.read();
                        let approval_ctx = tools::ToolExecutionContext::new(
                            tools::ExecutionScope::builder(
                                self.run_id.to_string(),
                                ws_read.workspace_id(),
                                ws_read.current_workspace_root(),
                            )
                            .build(),
                            tools::ToolExecutionPorts::new(
                                crate::application::runtime_context::tool_cancellation_signal(
                                    self.cancel_token(),
                                ),
                                crate::application::workspace_access::RuntimeWorkspaceAccess::new(
                                    self.workspace.clone(),
                                )
                                .read_access(),
                                Arc::new(tools::MutexReadSet(self.read_files.clone())),
                                Arc::new(tools::FixedPlanMode(None)),
                                self.memory().clone(),
                                Arc::new(tools::FixedGuidance {
                                    language: self.language.to_string(),
                                }),
                            ),
                        );
                        let approval_step_id = self
                            .context_request
                            .as_ref()
                            .map(|request| request.step_id.clone())
                            .unwrap_or_else(sdk::RunStepId::new_v7);
                        let domain = Self::make_agent(
                            self.tool_catalog(),
                            self.tool_execution(),
                            self.agent_runner,
                            self.memory(),
                            self.language,
                            &self.run_config().config().skills().dirs,
                            self.run_config().config().user_agent(),
                            self.workspace,
                            &self.cancel_token(),
                            self.read_files,
                            self.session_reminders,
                            self.max_tool_concurrency,
                            self.agent_semaphore,
                            self.session_id,
                            self.runtime_context.context(),
                            self.runtime_context.skill_load_state(),
                            &self.run_id,
                            self.run_config().tool_selection(),
                        )
                        .execute_domain_with_ctx(
                            &crate::application::subagent::ToolCall {
                                id: call.id.clone(),
                                provider_id: call.provider_id.clone(),
                                name: call.name.clone(),
                                index: call.index,
                                input: call.input.clone(),
                            },
                            &approval_ctx,
                            approval_call.authorization,
                            &approval_step_id,
                        )
                        .await;
                        let outcome = crate::application::subagent::legacy_outcome(domain);
                        let execution = ToolExecution::from_parts(
                            id.clone(),
                            call.provider_id.clone(),
                            call.name.clone(),
                            outcome.clone(),
                        );
                        let materializer = self.tool_result_materializer;
                        let msg =
                            crate::application::main_loop::looping::tools::tool_results_for_api(
                                materializer,
                                vec![execution],
                                self.session_id,
                            )
                            .await;
                        self.messages.push(msg.clone());
                        self.step_messages.record(msg);
                        self.mark_tool_results_pending();
                        let status = if outcome.is_error {
                            ToolCallStatus::Error
                        } else {
                            ToolCallStatus::Success
                        };
                        (id.clone(), status)
                    } else {
                        // No approval_call stored — treat as error
                        let outcome =
                            tools::ToolOutcome::error("tool approval call data not found");
                        let execution = ToolExecution::from_parts(
                            id.clone(),
                            String::new(),
                            "ToolApproval".to_string(),
                            outcome,
                        );
                        let materializer = self.tool_result_materializer;
                        let msg =
                            crate::application::main_loop::looping::tools::tool_results_for_api(
                                materializer,
                                vec![execution],
                                self.session_id,
                            )
                            .await;
                        self.messages.push(msg.clone());
                        self.step_messages.record(msg);
                        (id.clone(), ToolCallStatus::Error)
                    }
                } else {
                    // Denied — no execution, mark Cancelled
                    (id.clone(), ToolCallStatus::Cancelled)
                }
            }
            (
                InteractionContinuation::ContinueToolApproval(id),
                InteractionCompletion::Cancelled(_),
            ) => (id.clone(), ToolCallStatus::Cancelled),
            _ => {
                // HardPause / PlanApproval: no tool call work needed
                return Ok(
                    crate::application::loop_engine::InteractionWorkOutcome::Completed {
                        call_id: sdk::ToolCallId::new_v7(),
                        status: ToolCallStatus::Success,
                        remaining_queue,
                    },
                );
            }
        };

        Ok(
            crate::application::loop_engine::InteractionWorkOutcome::Completed {
                call_id,
                status,
                remaining_queue,
            },
        )
    }

    async fn on_stuck(
        &mut self,
        decision: &crate::application::loop_engine::StuckDecision,
    ) -> Result<(), LoopEngineError> {
        let _ = decision;
        Ok(())
    }

    fn needs_plan_approval(&self) -> bool {
        self.plan_mode
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

    fn take_control(&self, run_id: &sdk::RunId) -> Option<crate::domain::agent_run::RunControl> {
        debug_assert_eq!(run_id, &self.run_id);
        self.active_run.take_control(run_id)
    }

    fn claim_terminal(&self, run_id: &sdk::RunId) -> bool {
        self.active_run.claim_terminal(run_id)
    }

    fn claim_cancellation(&self, run_id: &sdk::RunId) -> bool {
        self.active_run.claim_cancellation(run_id)
    }

    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
        // #1385 Task 12: Route through RuntimeContext's event_sink, not self.sink.
        let mut strategy = MainEventStrategy {
            sink: self.runtime_context.event_sink(),
            session_id: self.session_id,
            turn_context: &self.turn_context,
            task_access: self.task_access(),
            model: &self.binding().model.model,
            started_at: self.started_at,
            turn_count: self.turn_count,
            messages_snapshot: self.messages.clone(),
        };
        strategy.emit(events).await
    }
}

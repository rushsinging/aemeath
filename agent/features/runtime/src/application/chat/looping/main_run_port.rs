use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::application::agent::runner::{log_agent_outcome, AgentRunOutcome, AgentRunStatus};
use crate::application::agent::{Agent, ToolCall};
use crate::application::chat::looping::finalize::{
    finish_completed_loop, run_stop_hook_before_finish,
};
use crate::application::chat::looping::hook_ui::HookUi;
use crate::application::chat::looping::llm_log::{log_llm_input, log_llm_output_and_tool_calls};
use crate::application::chat::looping::post_batch::run_post_tool_batch;
use crate::application::chat::looping::reflection::{
    maybe_submit_pre_compact_reflection, should_run_turn_reflection, submit_interval_reflection,
};
use crate::application::chat::looping::stream_handler::{
    should_emit_model_stream_waiting, InvocationEventReducer,
};
use crate::application::chat::looping::task_reminder::TaskReminderState;
use crate::application::chat::looping::tools::{execute_tool_round, tool_results_for_api};
use crate::application::chat::looping::{
    ChatEventSink, InputEventDrainPort, PendingInputBuffer, QueueDrainPort, RuntimeStreamEvent,
    RuntimeTurnContext,
};
use crate::application::context_coordination::ContextCoordinator;
use crate::application::loop_engine::{
    split_input_events, LoopEngineError, LoopInput, ModelStep, RunLoopPort, ToolGuardDecision,
    ToolStep,
};
use crate::domain::agent_run::RunDomainEvent;
use crate::ports::{
    CalendarDate, ContextRequest, ContextRequestId, Language as ContextLanguage, RunStepId,
    SessionId, SystemPromptSpec, TaskReminderSnapshot,
};
use workflow::api::{ReasoningPort, ReasoningSignal};

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
}

impl StepMessageOwnership {
    pub(crate) fn new(pending: Vec<Message>) -> Self {
        Self {
            pending,
            active: Vec::new(),
        }
    }

    fn freeze(&mut self, inputs: &[LoopInput]) -> Vec<Message> {
        let messages = if inputs.is_empty() {
            std::mem::take(&mut self.pending)
        } else {
            inputs
                .iter()
                .map(|input| Message::user(input.text.clone()))
                .collect()
        };
        self.active = messages.clone();
        messages
    }

    fn record(&mut self, message: Message) {
        self.active.push(message);
    }

    fn rollback_last(&mut self) -> Option<Message> {
        self.active.pop()
    }

    fn finalized(&self) -> Vec<Message> {
        self.active.clone()
    }

    fn committed(&mut self) {
        self.active.clear();
    }
}

#[cfg(test)]
pub(crate) fn fixture_bind_pending(
    pending: Vec<Message>,
    inputs: &[LoopInput],
) -> (Vec<Message>, Vec<Message>) {
    let mut ownership = StepMessageOwnership::new(pending);
    let frozen = ownership.freeze(inputs);
    (frozen, ownership.finalized())
}

#[cfg(test)]
pub(crate) fn fixture_finalize_messages(
    pending: Vec<Message>,
    produced: Vec<Message>,
) -> Vec<Message> {
    let mut ownership = StepMessageOwnership::new(pending);
    ownership.freeze(&[]);
    for message in produced {
        ownership.record(message);
    }
    ownership.finalized()
}

/// Main-chat adapter for the shared run loop.
///
/// It owns no lifecycle state machine. `Run` is the only per-run state machine; this adapter
/// projects its domain events and bridges the existing provider/tool/compact/hook helpers.
#[allow(clippy::too_many_arguments)]
pub(crate) struct MainRunPort<'a, S, Q, I>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    pub(crate) sink: &'a S,
    pub(crate) queue: &'a Q,
    pub(crate) input_events: &'a I,
    pub(crate) binding: &'a Arc<crate::ports::ProviderBinding>,
    pub(crate) tool_catalog: &'a Arc<dyn tools::ToolCatalogPort>,
    pub(crate) tool_execution: &'a Arc<dyn tools::ToolExecutionPort>,
    pub(crate) tool_context_binding: &'a Arc<dyn tools::ToolExecutionContextBindingPort>,
    pub(crate) system_prompt_text: &'a str,
    pub(crate) config_snapshot: &'a share::config::domain::snapshot::ConfigSnapshot,
    pub(crate) context: &'a ContextCoordinator,
    pub(crate) context_request: Option<crate::ports::ContextRequest>,
    pub(crate) context_window: Option<crate::ports::ContextWindow>,
    /// 当前 RunStep 的显式消息所有权；历史长度不参与归属判断。
    pub(crate) step_messages: StepMessageOwnership,
    pub(crate) messages: Vec<Message>,
    pub(crate) context_size: usize,
    pub(crate) workspace: &'a project::WorkspaceViews,
    pub(crate) session_id: &'a str,
    pub(crate) context_session_id: &'a str,
    pub(crate) read_files: &'a Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub(crate) session_reminders: &'a Arc<std::sync::Mutex<tools::SessionReminders>>,
    pub(crate) agent_runner: &'a Option<Arc<dyn tools::AgentRunner>>,
    pub(crate) tool_result_materializer:
        &'a crate::application::tool_result_materialization::ToolResultMaterializer,
    pub(crate) policy: &'a dyn policy::PolicyPort,
    /// Runtime/Tool 日常状态唯一来源（#889 low-privilege 端口）。
    pub(crate) task_access: &'a Arc<dyn task::TaskAccess>,
    pub(crate) max_tool_concurrency: usize,
    pub(crate) agent_semaphore: &'a Arc<tokio::sync::Semaphore>,
    pub(crate) hook_runner: &'a hook::api::HookRunner,
    pub(crate) memory_config: &'a share::config::MemoryConfig,
    pub(crate) memory: &'a Arc<dyn memory::MemoryPort>,
    pub(crate) reflection_history: &'a Arc<dyn memory::api::ReflectionHistoryStore>,
    pub(crate) reflection_tasks: &'a crate::application::reflection::ReflectionTaskAdapter,
    pub(crate) language: &'a str,
    pub(crate) reasoning: &'a dyn ReasoningPort,
    pub(crate) pending_input: &'a mut PendingInputBuffer,
    pub(crate) deferred_user_inputs: &'a mut VecDeque<sdk::ChatInputEvent>,
    pub(crate) cancel: CancellationToken,
    pub(crate) run_id: sdk::RunId,
    pub(crate) active_run: &'a dyn crate::domain::agent_run::ActiveRunPort,
    pub(crate) turn_count: usize,
    pub(crate) turn_context: RuntimeTurnContext,
    pub(crate) last_total_tokens: &'a mut Option<u64>,
    pub(crate) task_reminder_state: &'a mut TaskReminderState,
    pub(crate) tool_identity:
        &'a crate::application::tool_coordination::identity::ToolIdentityRegistry,
    pub(crate) started_at: Instant,
}

impl<S, Q, I> MainRunPort<'_, S, Q, I>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn freeze_request(
        &self,
        step_id: &RunStepId,
        pending_messages: Vec<Message>,
    ) -> ContextRequest {
        let task_reminder = self
            .task_access
            .reminder_snapshot()
            .items
            .iter()
            .any(|item| {
                matches!(
                    item.status,
                    task::TaskStatus::Pending | task::TaskStatus::InProgress
                )
            })
            .then(|| "当前 task batch 仍有未完成任务；仅在与最新用户请求相关时继续。".to_string());
        let raw_tool_schemas = self
            .tool_catalog
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
            session_id: SessionId::new(self.context_session_id),
            request_id: ContextRequestId::new(uuid::Uuid::now_v7().to_string()),
            run_id: self.run_id.clone(),
            step_id: step_id.clone(),
            pending_messages,
            system_prompt: SystemPromptSpec::new(self.system_prompt_text),
            model_id: self.binding.model.model.clone(),
            effective_reasoning: self.reasoning.current_requested_level(),
            current_date: CalendarDate::new(chrono::Local::now().format("%Y-%m-%d").to_string()),
            task_reminder: TaskReminderSnapshot {
                text: task_reminder,
            },
            language: ContextLanguage::new(self.language),
            agent_roles: std::collections::HashMap::new(),
            config_snapshot: self.config_snapshot.clone(),
            context_size: self.context_size,
            max_output_tokens: self.binding.max_tokens as usize,
            last_api_input_tokens: *self.last_total_tokens,
            tool_schemas,
            tool_schema_tokens: context::compact::estimate_tool_schemas_tokens(&raw_tool_schemas),
            prev_system_tokens: None,
            prev_tool_schema_tokens: None,
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
        let messages = self.step_messages.finalized();
        self.context
            .append_finalized(
                request,
                request.step_id.clone(),
                window.backing_revision,
                cause,
                messages,
                vec![],
                *self.last_total_tokens,
            )
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        self.step_messages.committed();
        Ok(())
    }

    async fn queue_busy_event(&mut self, event: sdk::ChatInputEvent) {
        match event {
            sdk::ChatInputEvent::UserMessage { .. } => {
                self.deferred_user_inputs.push_back(event);
                let queued = self
                    .deferred_user_inputs
                    .iter()
                    .filter_map(|event| match event {
                        sdk::ChatInputEvent::UserMessage { id, text, .. } => {
                            Some((id.clone(), Message::user(text.clone())))
                        }
                        _ => None,
                    })
                    .collect();
                self.sink
                    .send_event(RuntimeStreamEvent::UserMessagesQueued { queued })
                    .await;
            }
            sdk::ChatInputEvent::WithdrawAll => {
                let texts = self
                    .deferred_user_inputs
                    .drain(..)
                    .filter_map(|event| match event {
                        sdk::ChatInputEvent::UserMessage { text, .. } => Some(text),
                        _ => None,
                    })
                    .collect();
                self.sink
                    .send_event(RuntimeStreamEvent::UserMessagesWithdrawn { texts })
                    .await;
            }
            // Commands are retained for the session idle gate. They never enter model context.
            other => self.pending_input.push(other),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn make_agent(
        tool_catalog: &Arc<dyn tools::ToolCatalogPort>,
        tool_execution: &Arc<dyn tools::ToolExecutionPort>,
        agent_runner: &Option<Arc<dyn tools::AgentRunner>>,
        memory: &Arc<dyn memory::MemoryPort>,
        language: &str,
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
                    crate::application::tool_execution_adapters::cancellation(cancel.clone()),
                    crate::application::tool_execution_adapters::RuntimeWorkspaceAccess::new(
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

    fn outcome(&self, status: AgentRunStatus) -> AgentRunOutcome {
        AgentRunOutcome {
            status,
            turns: self.turn_count,
            duration: self.started_at.elapsed(),
            role: None,
            model: self.binding.model.model.clone(),
        }
    }

    async fn project_done(&self, status: AgentRunStatus) {
        let outcome = self.outcome(status);
        log_agent_outcome(&outcome, self.session_id);
        finish_completed_loop(&outcome, self.sink, &self.turn_context, &**self.task_access).await;
    }

    async fn rollback_cancelled(&mut self) {
        self.sink
            .send_event(RuntimeStreamEvent::Cancelled {
                context: self.turn_context.clone(),
            })
            .await;
    }
    async fn invoke_model_impl(
        &mut self,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        if self.context_window.is_none() {
            if let Some(request) = &self.context_request {
                let window = self
                    .context
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
        self.task_reminder_state
            .update_from_messages(self.turn_count as u64, &window.messages);
        let messages_for_api = window
            .messages
            .iter()
            .map(Message::to_llm_view)
            .collect::<Vec<_>>();
        let tool_schemas = window
            .tool_schemas
            .iter()
            .map(|schema| {
                serde_json::json!({
                    "name": schema.name,
                    "description": schema.description,
                    "input_schema": schema.input_schema,
                })
            })
            .collect::<Vec<_>>();
        let effective_system_blocks = window
            .system_blocks
            .iter()
            .map(|block| {
                if block.cacheable {
                    provider::RequestSystemBlock::Cacheable(block.content.clone())
                } else {
                    provider::RequestSystemBlock::Text(block.content.clone())
                }
            })
            .collect::<Vec<_>>();
        log_llm_input(
            &messages_for_api,
            window.messages.len(),
            &effective_system_blocks,
            &tool_schemas,
        );
        let requested_reasoning = self.reasoning.current_requested_level();

        let api_start = Instant::now();
        let mut coordinator =
            crate::application::model_invocation::ModelInvocationCoordinator::new();
        let resp = loop {
            let request_context = request_log_context(
                &logging::capture(),
                self.binding.model.model.as_str(),
                self.binding.model.provider.as_str(),
                "default",
            );
            let mut reducer = InvocationEventReducer::with_tool_identity(
                self.sink.clone(),
                self.tool_identity.clone(),
                self.turn_context.clone(),
            );
            let response = logging::instrument(request_context.clone(), async {
                let progress_handle = reducer.progress_handle();
                let stream_cancel = self.cancel.clone();
                let provider = self.binding.provider.clone();
                let model = self.binding.model.clone();
                let max_tokens = self.binding.max_tokens;
                let request_tool_schemas = window.tool_schemas.clone();
                let invocation_fut = async {
                    let mut request = crate::ports::InvocationRequest::new(
                        model,
                        messages_for_api.clone(),
                        crate::ports::InvocationOptions::new(max_tokens, requested_reasoning),
                    );
                    request.system = effective_system_blocks.clone();
                    request.tools = request_tool_schemas;
                    request.cancellation = stream_cancel.clone();
                    let stream = provider
                        .invoke(request, &stream_cancel)
                        .await
                        .map_err(|error| (error, false))?;
                    coordinator
                        .pull_stream(stream, &stream_cancel, true, |event| reducer.apply(event))
                        .await
                };
                let waiting_sink = self.sink.clone();
                let waiting_context = self.turn_context.clone();
                let request_started_at = tokio::time::Instant::now();
                let waiting_task =
                    AbortTaskOnDrop(logging::spawn_instrumented(request_context, async move {
                        let mut next = request_started_at + Duration::from_secs(10);
                        let mut last_version = None;
                        loop {
                            tokio::time::sleep_until(next).await;
                            let snapshot = progress_handle.lock().unwrap().snapshot();
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
                Err((error, _)) if error.is_cancelled() || self.cancel.is_cancelled() => {
                    return Err(LoopEngineError::Cancelled);
                }
                Err((error, visible_delta)) => match coordinator
                    .handle_failure(&error, visible_delta, &self.cancel)
                    .await
                {
                    crate::application::model_invocation::RetryStep::Retry { attempt, delay } => {
                        self.sink
                            .try_send_event(RuntimeStreamEvent::ModelInvocationRetrying {
                                context: self.turn_context.clone(),
                                attempt,
                                delay,
                            });
                    }
                    crate::application::model_invocation::RetryStep::Cancelled => {
                        return Err(LoopEngineError::Cancelled);
                    }
                    crate::application::model_invocation::RetryStep::Compact => {
                        return Err(LoopEngineError::NeedsCompaction(error.to_string()));
                    }
                    crate::application::model_invocation::RetryStep::Fail => {
                        return Err(LoopEngineError::Adapter(error.to_string()));
                    }
                },
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

        *self.last_total_tokens = Some(crate::application::token_usage::normalized_total_tokens(
            &resp.usage,
        ));

        let token_usage = crate::application::loop_engine::StepTokenUsage {
            input_tokens: resp.usage.input_tokens.unwrap_or(0) as u64,
            output_tokens: resp.usage.output_tokens.unwrap_or(0) as u64,
            cached_tokens: resp.usage.cache_read_tokens.map(u64::from).unwrap_or(0),
            cache_creation_tokens: resp.usage.cache_write_tokens.map(u64::from).unwrap_or(0),
            reasoning_tokens: resp.usage.reasoning_tokens.map(u64::from).unwrap_or(0),
            total_tokens: crate::application::token_usage::normalized_total_tokens(&resp.usage),
            context_window: request_context_size(self.context_request.as_ref()) as u64,
            est_system_tokens: window.token_estimation.system_tokens,
            est_tool_tokens: window.token_estimation.tool_schema_tokens,
            est_message_tokens: window.token_estimation.message_tokens,
            stop_reason: format!("{:?}", resp.stop_reason).to_lowercase(),
        };

        self.sink
            .send_event(RuntimeStreamEvent::Usage {
                input: resp.usage.input_tokens.unwrap_or(0),
                output: resp.usage.output_tokens.unwrap_or(0),
                last_input: resp.usage.input_tokens.unwrap_or(0),
                elapsed_secs: api_elapsed,
            })
            .await;
        self.messages.push(resp.assistant_message.clone());
        self.step_messages.record(resp.assistant_message.clone());
        self.sink
            .send_event(RuntimeStreamEvent::TurnStarted {
                messages: self.messages.clone(),
            })
            .await;

        let calls = Agent::extract_tool_calls_with_ids(&resp.assistant_message, |provider_id| {
            self.tool_identity.runtime_id_for_provider(provider_id)
        });
        log_llm_output_and_tool_calls(
            self.binding.model.provider.as_str(),
            &resp,
            &calls,
            api_elapsed,
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

        let observation = self.reasoning.observe(ReasoningSignal::TextOnly);
        if observation.changed() {
            self.sink
                .send_event(RuntimeStreamEvent::GraphPhaseChanged {
                    node: observation.current,
                    effort: observation.requested,
                    prev: observation.previous,
                })
                .await;
        }
        if should_run_turn_reflection(
            self.memory_config,
            self.turn_count,
            !calls.is_empty(),
            &resp.stop_reason,
            false,
        ) {
            let _ = submit_interval_reflection(
                self.reflection_tasks,
                self.memory_config,
                self.turn_count,
                &self.messages,
                self.binding,
                self.system_prompt_text,
                self.language,
                self.memory,
                self.reflection_history,
            );
        }

        let outcome = self.outcome(AgentRunStatus::Completed);
        if let Some(feedback) = run_stop_hook_before_finish(
            &outcome,
            self.sink,
            &HookUi::new(self.sink.clone()),
            self.hook_runner,
            self.session_id,
            self.language,
            &self.current_cwd(),
            &self.cancel,
        )
        .await
        {
            let blocked_assistant = self.messages.pop();
            let blocked_step_assistant = self.step_messages.rollback_last();
            debug_assert!(blocked_assistant.is_some());
            debug_assert!(blocked_step_assistant.is_some());
            let feedback = Message::system_generated_user(format!(
                "<system-reminder>\n{feedback}\n</system-reminder>"
            ));
            self.messages.push(feedback.clone());
            self.step_messages.record(feedback);
            self.sink
                .send_event(RuntimeStreamEvent::StopHookBlocked {
                    messages: self.messages.clone(),
                })
                .await;
            return Ok((
                ModelStep::StopHookBlocked {
                    text: resp.assistant_message.text_content(),
                },
                token_usage,
            ));
        }
        Ok((
            ModelStep::Complete {
                text: resp.assistant_message.text_content(),
            },
            token_usage,
        ))
    }
}

#[async_trait]
impl<S, Q, I> RunLoopPort for MainRunPort<'_, S, Q, I>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    fn freeze_step(&mut self, step_id: &RunStepId, inputs: &[LoopInput]) {
        let pending_messages = self.step_messages.freeze(inputs);
        if !inputs.is_empty() {
            self.messages.extend(pending_messages.clone());
        }
        self.context_request = Some(self.freeze_request(step_id, pending_messages));
        self.context_window = None;
    }

    async fn drain_input(&mut self) -> Result<Vec<LoopInput>, LoopEngineError> {
        let mut events = self.input_events.drain_input_events().await;
        if let Some(queued) = self.queue.drain_queued_input().await {
            events.extend(
                queued
                    .into_iter()
                    .map(|text| sdk::ChatInputEvent::classify_text(text, Vec::new())),
            );
        }
        let batch = split_input_events(events.clone());
        let inputs = batch
            .user_inputs
            .into_iter()
            .map(|input| LoopInput { text: input.text })
            .collect();
        for event in events {
            self.queue_busy_event(event).await;
        }
        Ok(inputs)
    }

    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError> {
        let request = self
            .context_request
            .as_ref()
            .ok_or_else(|| LoopEngineError::Adapter("ContextRequest 尚未冻结".to_string()))?;
        let window = self
            .context
            .build_window(request)
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        let needed = self
            .context
            .needs_compaction(request)
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        self.context_window = Some(window);
        Ok(needed)
    }

    async fn compact(&mut self, _cancel: &CancellationToken) -> Result<(), LoopEngineError> {
        let request = self
            .context_request
            .as_ref()
            .ok_or_else(|| LoopEngineError::Adapter("ContextRequest 尚未冻结".to_string()))?;
        let source_revision = self
            .context_window
            .as_ref()
            .map(|window| window.backing_revision)
            .ok_or_else(|| LoopEngineError::Adapter("ContextWindow 尚未构建".to_string()))?;
        // Freeze the pre-compact messages snapshot before invoking the context
        // adapter. Only the early window that compact will discard feeds the
        // PreCompact reflection; the recent tail stays in `recent_messages` and
        // is observable by the next LLM turn without going through Memory.
        let pre_compact_snapshot: Vec<share::message::Message> = self
            .context_window
            .as_ref()
            .map(|window| {
                context::compact::messages_selected_for_precompact_memory(&window.messages)
            })
            .unwrap_or_default();
        let outcome = self
            .context
            .compact(request, source_revision)
            .await
            .map_err(|error| LoopEngineError::Adapter(error.to_string()))?;
        // Production PreCompact reflection trigger (#1284): only the success
        // path submits the frozen pre-compact snapshot. Errors and `Skipped`
        // never enqueue a job. The submission is non-blocking and shares the
        // session-scoped slot with Interval and Manual triggers; the helper
        // returns `BusySkipped`/`DisabledSkipped` without blocking the caller.
        let _ = maybe_submit_pre_compact_reflection(
            &outcome,
            &pre_compact_snapshot,
            self.reflection_tasks,
            self.memory_config,
            self.binding,
            self.system_prompt_text,
            self.language,
            self.memory,
            self.reflection_history,
        );
        match outcome {
            crate::ports::CompactOutcome::Committed(_) => {
                *self.last_total_tokens = None;
                self.context_window = None;
                Ok(())
            }
            crate::ports::CompactOutcome::Skipped(reason) => Err(LoopEngineError::Adapter(
                format!("Context compact 被跳过：{reason:?}"),
            )),
        }
    }

    async fn invoke_model(
        &mut self,
        _cancel: &CancellationToken,
    ) -> Result<(ModelStep, crate::application::loop_engine::StepTokenUsage), LoopEngineError> {
        self.invoke_model_impl().await
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
        if cancel.is_cancelled() {
            return Err(LoopEngineError::Cancelled);
        }
        if calls.is_empty() {
            return Ok(ToolStep::Continue);
        }
        let raw_calls: Vec<_> = calls.iter().map(|(call, _)| call.clone()).collect();
        let agent = Self::make_agent(
            self.tool_catalog,
            self.tool_execution,
            self.agent_runner,
            self.memory,
            self.language,
            self.workspace,
            &self.cancel,
            self.read_files,
            self.session_reminders,
            self.max_tool_concurrency,
            self.agent_semaphore,
            self.session_id,
            &self.run_id,
        );
        let _binding = tools::ToolExecutionContextBindingGuard::bind(
            (*self.tool_context_binding).clone(),
            agent.ctx.clone(),
        )
        .map_err(LoopEngineError::Adapter)?;
        let (all_results, fuse_bypassed) = execute_tool_round(
            &self.turn_context,
            &raw_calls,
            self.tool_catalog,
            self.tool_execution,
            self.policy,
            run_id,
            step_id,
            &agent,
            self.sink,
            &HookUi::new(self.sink.clone()),
            self.hook_runner,
            cancel,
            self.language,
            &self.current_cwd(),
            calls,
        )
        .await;
        if cancel.is_cancelled() {
            return Err(LoopEngineError::Cancelled);
        }

        let metadata: HashMap<&str, (Option<&str>, Option<&str>)> = raw_calls
            .iter()
            .map(|call| {
                let command = (call.name == "Bash")
                    .then(|| call.input.get("command").and_then(|value| value.as_str()))
                    .flatten();
                let phase = call.input.get("phase").and_then(|value| value.as_str());
                (call.provider_id.as_str(), (command, phase))
            })
            .collect();
        for result in &all_results {
            let (command, phase) = metadata
                .get(result.provider_id.as_str())
                .copied()
                .unwrap_or((None, None));
            let observation = self.reasoning.observe(ReasoningSignal::ToolCompleted {
                tool_name: result.tool_name.clone(),
                bash_command: command.map(str::to_string),
                is_error: result.outcome.is_error,
                declared_phase: phase.map(str::to_string),
            });
            if observation.changed() {
                self.sink
                    .send_event(RuntimeStreamEvent::GraphPhaseChanged {
                        node: observation.current,
                        effort: observation.requested,
                        prev: observation.previous,
                    })
                    .await;
            }
        }
        let has_task_mutation = all_results.iter().any(|result| {
            crate::application::chat::looping::events::is_task_store_mutation(&result.tool_name)
        });
        let tool_results =
            tool_results_for_api(self.tool_result_materializer, all_results, self.session_id).await;
        self.messages.push(tool_results.clone());
        self.step_messages.record(tool_results);
        self.sink
            .send_event(RuntimeStreamEvent::PostToolExecutionSync {
                messages: self.messages.clone(),
            })
            .await;
        if has_task_mutation {
            let snapshot = crate::application::chat::looping::task_snapshot::build_task_snapshot(
                &**self.task_access,
            );
            self.sink
                .send_event(RuntimeStreamEvent::TasksSnapshot {
                    tasks: Box::new(snapshot),
                })
                .await;
        }
        run_post_tool_batch(
            self.sink,
            &HookUi::new(self.sink.clone()),
            self.hook_runner,
            &agent.runtime_cancellation,
            self.turn_count,
            &self.current_cwd(),
        )
        .await;
        Ok(if fuse_bypassed.is_empty() {
            ToolStep::Continue
        } else {
            ToolStep::ContinueWithFuseBypass(fuse_bypassed)
        })
    }

    async fn on_stuck(
        &mut self,
        decision: &crate::application::loop_engine::StuckDecision,
    ) -> Result<(), LoopEngineError> {
        let _ = decision;
        Ok(())
    }

    fn claim_terminal(&self, run_id: &sdk::RunId) -> bool {
        debug_assert_eq!(run_id, &self.run_id);
        self.active_run.claim_terminal(run_id)
    }

    fn claim_cancellation(&self, run_id: &sdk::RunId) -> bool {
        debug_assert_eq!(run_id, &self.run_id);
        self.active_run.claim_cancellation(run_id)
    }

    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError> {
        for event in events {
            match event {
                RunDomainEvent::Completed { .. } => {
                    self.project_done(AgentRunStatus::Completed).await;
                }
                RunDomainEvent::Failed { error, .. } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::ApiError {
                            messages: self.messages.clone(),
                            error: error.clone(),
                        })
                        .await;
                    self.project_done(AgentRunStatus::ApiError(error)).await;
                }
                RunDomainEvent::Cancelled { run_id, .. } => {
                    self.rollback_cancelled().await;
                    self.sink
                        .send_event(RuntimeStreamEvent::RunCancelled { run_id })
                        .await;
                }
                RunDomainEvent::Terminated { run_id, .. } => {
                    self.rollback_cancelled().await;
                    self.sink
                        .send_event(RuntimeStreamEvent::RunCancelled { run_id })
                        .await;
                }
                RunDomainEvent::CancellationRequested { run_id, .. } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::RunCancelling { run_id })
                        .await;
                }
                RunDomainEvent::Started {
                    run_id,
                    parent_run_id,
                } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::RunStarted {
                            run_id,
                            parent_run_id,
                        })
                        .await;
                }
                RunDomainEvent::StuckDetected { reason, .. } => {
                    self.sink
                        .send_event(RuntimeStreamEvent::SystemMessage(format!(
                            "[StuckGuard: {reason}]"
                        )))
                        .await;
                }
                RunDomainEvent::Transitioned { .. }
                | RunDomainEvent::AwaitingUser { .. }
                | RunDomainEvent::Resumed { .. }
                | RunDomainEvent::StepStarted { .. }
                | RunDomainEvent::StepCompleted { .. }
                | RunDomainEvent::StepCancellationRequested { .. }
                | RunDomainEvent::StepFinalizationStarted { .. }
                | RunDomainEvent::StepCancelled { .. }
                | RunDomainEvent::DrainingInput { .. }
                | RunDomainEvent::TerminationRequested { .. } => {
                    self.sink.send_domain_event(event).await;
                }
            }
        }
        Ok(())
    }
}

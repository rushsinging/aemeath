use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::application::loop_engine::chat::post_batch::run_post_tool_batch;
use crate::application::loop_engine::chat::reflection::{
    maybe_submit_pre_compact_reflection, should_run_turn_reflection, submit_interval_reflection,
};
use crate::application::loop_engine::chat::stream_handler::InvocationEventReducer;
use crate::application::loop_engine::chat::{
    ChatEventSink, RuntimeStreamEvent, RuntimeTurnContext,
};
use crate::application::loop_engine::event_strategy::{ChatStreamEventObserver, RunEventObserver};
use crate::application::loop_engine::input_strategy::{
    BufferedInputAdapter, InputContinuationState, SessionInputPort,
};
use crate::application::loop_engine::{EventSinkPort, LoopEngineError, ModelStep};
use crate::application::run::context::RuntimeContext;
use crate::application::run::execution_state::RunExecutionState;
use crate::application::tool::agent::{Agent, ToolCall};
use crate::domain::agent_run::RunDomainEvent;
use crate::ports::ContextRequest;

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
    let mut execution = RunExecutionState::new();
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
    let mut execution = RunExecutionState::new();
    execution.replace_pending_step_messages(pending);
    execution.freeze_step_input_messages(prefix, loop_input_messages(inputs));
    execution.accepted_input_snapshot()
}

#[cfg(test)]
pub(crate) fn fixture_finalize_messages(
    pending: Vec<Message>,
    produced: Vec<Message>,
) -> Vec<Message> {
    let mut execution = RunExecutionState::new();
    execution.freeze_step_messages(pending.clone());
    for message in produced {
        execution.record_step_message(message);
    }
    pending
        .into_iter()
        .chain(execution.step_outcome())
        .collect()
}

#[cfg(test)]
pub(crate) fn fixture_two_step_accepted(
    pending: Vec<Message>,
    first_inputs: &[crate::application::loop_engine::LoopInput],
    second_inputs: &[crate::application::loop_engine::LoopInput],
) -> (Vec<Message>, Vec<Message>) {
    let mut execution = RunExecutionState::new();
    execution.replace_pending_step_messages(pending);
    execution.freeze_step_input_messages(None, loop_input_messages(first_inputs));
    let first_accepted = execution.accepted_input_snapshot();
    execution.freeze_step_input_messages(None, loop_input_messages(second_inputs));
    let second_accepted = execution.accepted_input_snapshot();
    (first_accepted, second_accepted)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn make_agent(
    runtime_context: &RuntimeContext,
    agent_runner: Option<Arc<dyn tools::AgentRunner>>,
    language: &str,
    workspace: &project::WorkspaceViews,
    cancel: &CancellationToken,
    read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    session_reminders: Arc<std::sync::Mutex<tools::SessionReminders>>,
    max_tool_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    session_id: &str,
    run_id: &sdk::RunId,
    tool_result_materializer: Arc<
        crate::application::tool::tool_result_materializer::ToolResultMaterializer,
    >,
) -> Agent {
    let catalog = runtime_context
        .tool_catalog_ref()
        .snapshot(
            &tools::RegistryScopeName::new("main"),
            &tools::ToolProfileName::new("main-full"),
        )
        .unwrap_or_else(|_| tools::ToolCatalogSnapshot::new("main", "main-full", Vec::new()));
    let runtime_provider = config::resolve_provider_runtime_for_selection(
        runtime_context.config_ref().config(),
        &format!(
            "{}/{}",
            runtime_context.provider_ref().model.provider,
            runtime_context.provider_ref().model.model
        ),
        None,
    );
    Agent {
        catalog,
        execution: runtime_context.tool_execution(),
        context: crate::application::context::coordination::ContextCoordinator::new(
            runtime_context.context(),
        ),
        session_id: context::domain::SessionId::new(session_id),
        ctx: tools::ToolExecutionContext::new(
            tools::ExecutionScope::builder(
                run_id.to_string(),
                workspace.read().workspace_id(),
                workspace.read().current_workspace_root(),
            )
            .build(),
            tools::ToolExecutionPorts::new(
                crate::adapters::tool_runtime::cancellation(cancel.clone()),
                crate::application::run::workspace::RuntimeWorkspaceAccess::new(workspace.clone())
                    .read_access(),
                Arc::new(tools::MutexReadSet(read_files)),
                Arc::new(tools::FixedPlanMode(None)),
                runtime_context.memory(),
                Arc::new(tools::FixedGuidance {
                    language: language.to_string(),
                }),
            )
            .with_user_agent(&runtime_provider.user_agent)
            .with_memory_context(Some(session_id.to_string()), Some(session_reminders))
            .with_skill_load_state(
                tools::SkillLoadScope::main(),
                runtime_context.skill_load_state(),
            )
            .with_agent(agent_runner),
        ),
        max_tool_concurrency,
        agent_semaphore,
        workspace_persist: workspace.persist(),
        tool_result_materializer,
        runtime_cancellation: cancel.clone(),
    }
}

pub(crate) struct ChatEventPort {
    pub sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
    pub session_id: String,
    pub turn_context: RuntimeTurnContext,
    pub task_access: Arc<dyn task::TaskAccess>,
    pub model: String,
}

#[async_trait]
impl EventSinkPort for ChatEventPort {
    async fn emit(
        &mut self,
        execution: &mut RunExecutionState,
        events: Vec<RunDomainEvent>,
    ) -> Result<(), LoopEngineError> {
        ChatStreamEventObserver {
            sink: self.sink.clone(),
            session_id: &self.session_id,
            turn_context: &self.turn_context,
            task_access: &self.task_access,
            model: &self.model,
            started_at: execution.started_at().unwrap_or_else(Instant::now),
            turn_count: execution.turn_count(),
            messages_snapshot: execution.messages_snapshot(),
        }
        .emit(events)
        .await
    }
}

pub(crate) struct ChatAcceptedInputObserver {
    pub sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
    pub input: crate::application::run::context::RunInputBufferHandle,
}

#[async_trait]
impl crate::application::loop_engine::step_persistence::AcceptedInputObserver
    for ChatAcceptedInputObserver
{
    async fn on_accepted_input(
        &mut self,
        execution: &mut RunExecutionState,
    ) -> Result<(), LoopEngineError> {
        let adopted = execution.take_adopted_input();
        if adopted.is_empty() {
            return Ok(());
        }
        let queued = self
            .input
            .with_lock(|buffer| buffer.user_message_snapshot());
        self.sink
            .send_event(RuntimeStreamEvent::UserMessagesAdopted {
                items: adopted,
                queued,
            })
            .await;
        Ok(())
    }
}

pub(crate) struct ChatCompactionObserver {
    pub runtime_context: RuntimeContext,
    pub reflection_tasks: crate::application::reflection::ReflectionTaskAdapter,
    pub system_prompt: String,
    pub language: String,
}

#[async_trait]
impl crate::application::loop_engine::compaction::CompactionObserver for ChatCompactionObserver {
    async fn on_compacted(
        &mut self,
        outcome: &crate::ports::CompactOutcome,
        discarded_messages: &[Message],
    ) -> Result<(), LoopEngineError> {
        let _ = maybe_submit_pre_compact_reflection(
            outcome,
            discarded_messages,
            &self.reflection_tasks,
            self.runtime_context.config_ref().config().memory(),
            self.runtime_context.provider_ref(),
            &self.system_prompt,
            &self.language,
            self.runtime_context.memory_ref(),
            self.runtime_context.reflection_history_ref(),
        );
        Ok(())
    }
}

pub(crate) struct ChatStopHookObserver {
    pub sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
    pub continuation: InputContinuationState,
}

#[async_trait]
impl crate::application::hook::stop_coordination::StopHookObserver for ChatStopHookObserver {
    async fn begin_stop_hook_status(&mut self) -> Result<(), LoopEngineError> {
        use crate::application::loop_engine::chat::{RuntimeHookEvent, RuntimeHookEventStatus};
        self.sink
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

    fn install_stop_hook_feedback(&mut self, message: Message) {
        self.continuation.install_stop_hook_feedback(message);
    }

    async fn observe_stop_hook_outcome(
        &mut self,
        execution: &RunExecutionState,
        outcome: &crate::application::hook::stop_coordination::StopHookOutcome,
    ) -> Result<(), LoopEngineError> {
        crate::application::loop_engine::chat::hook_ui::project_hook_dispatch(
            &self.sink,
            outcome.point,
            &outcome.dispatch,
        )
        .await;
        if outcome.feedback_message.is_some() {
            self.sink
                .send_event(RuntimeStreamEvent::StopHookBlocked {
                    messages: execution.messages_snapshot(),
                })
                .await;
        }
        Ok(())
    }
}

pub(crate) struct ChatToolRoundObserver {
    pub runtime_context: RuntimeContext,
    pub workspace_root: std::path::PathBuf,
}

#[async_trait]
impl crate::application::tool::coordination::ToolRoundObserver for ChatToolRoundObserver {
    async fn results_materialized(
        &mut self,
        execution: &RunExecutionState,
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

pub(crate) struct ChatModelObserver<I>
where
    I: SessionInputPort,
{
    pub runtime_context: RuntimeContext,
    pub input: BufferedInputAdapter<I>,
    pub system_prompt: String,
    pub context_size: usize,
    pub reflection_tasks: crate::application::reflection::ReflectionTaskAdapter,
    pub language: String,
    pub turn_context: RuntimeTurnContext,
    pub tool_identity: crate::application::tool::coordination::identity::ToolIdentityRegistry,
}

impl<I> ChatModelObserver<I>
where
    I: SessionInputPort,
{
    async fn queue_busy_event(&mut self, event: sdk::ChatInputEvent) {
        match event {
            sdk::ChatInputEvent::UserMessage { .. } => self.input.admit_user_message(event).await,
            sdk::ChatInputEvent::WithdrawAll => {
                let texts = self
                    .input
                    .run_input_buffer
                    .with_lock(|buffer| buffer.withdraw_all_user_texts());
                self.runtime_context
                    .event_sink()
                    .send_event(RuntimeStreamEvent::UserMessagesWithdrawn { texts })
                    .await;
            }
            other => self.input.pending_input.push(other),
        }
    }
}

impl<I> crate::application::model::invocation::ModelInvocationSource for ChatModelObserver<I>
where
    I: SessionInputPort,
{
    fn runtime_context(&self) -> &RuntimeContext {
        &self.runtime_context
    }

    fn role(&self) -> &str {
        "main"
    }

    fn request_log_context(&self, parent: &logging::LogContext) -> logging::LogContext {
        request_log_context(
            parent,
            &self.runtime_context.provider_ref().model.model,
            &self.runtime_context.provider_ref().model.provider,
            "default",
        )
    }

    fn context_size(&self, execution: &RunExecutionState) -> usize {
        execution
            .context_request()
            .map_or(self.context_size.max(1), |request| {
                request_context_size(Some(request))
            })
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
impl<I> crate::application::model::invocation::ModelInvocationObserver for ChatModelObserver<I>
where
    I: SessionInputPort,
{
    async fn pump_while_invoking<T: Send>(
        &mut self,
        invocation: impl std::future::Future<Output = T> + Send,
    ) -> T {
        tokio::pin!(invocation);
        loop {
            tokio::select! {
                response = &mut invocation => break response,
                event = self.input.input_events.recv_next_input() => {
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
        execution: &mut RunExecutionState,
        response: &crate::application::loop_engine::chat::InvocationResponse,
        elapsed_secs: f64,
    ) {
        for event in self.input.input_events.drain_input_events().await {
            self.queue_busy_event(event).await;
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
        execution: &mut RunExecutionState,
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
        let memory_config = self.runtime_context.config_ref().config().memory();
        if should_run_turn_reflection(
            memory_config,
            execution.turn_count(),
            false,
            &response.stop_reason,
            false,
        ) {
            let _ = submit_interval_reflection(
                &self.reflection_tasks,
                memory_config,
                execution.turn_count(),
                execution.messages(),
                self.runtime_context.provider_ref(),
                &self.system_prompt,
                &self.language,
                self.runtime_context.memory_ref(),
                self.runtime_context.reflection_history_ref(),
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

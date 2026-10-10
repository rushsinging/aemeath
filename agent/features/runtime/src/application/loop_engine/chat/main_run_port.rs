use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::application::loop_engine::chat::post_batch::run_post_tool_batch;
use crate::application::loop_engine::chat::stream_handler::InvocationEventReducer;
use crate::application::loop_engine::chat::{ChatEventSink, RuntimeRunContext, RuntimeStreamEvent};
use crate::application::loop_engine::event_strategy::{ChatStreamEventObserver, RunEventObserver};
use crate::application::loop_engine::input_strategy::{
    BufferedInputAdapter, InputContinuationState, SessionInputPort,
};
use crate::application::loop_engine::{EventSinkPort, LoopEngineError, ModelStep};
use crate::application::run::context::RuntimeContext;
use crate::application::run::execution_state::RunExecutionState;
use crate::application::tool::agent::{Agent, ToolCall};
use crate::domain::agent_run::RuntimeLifecycleEvent;
use crate::ports::ContextRequestData;

fn request_context_size(request: Option<&ContextRequestData>) -> usize {
    request.map_or(1, |request| request.context_size.max(1))
}

pub(crate) fn request_log_context(
    parent: &logging::LogContext,
    model: &str,
    provider: &str,
    role: &str,
) -> logging::LogContext {
    parent.patched(logging::LogContextPatch {
        request_id: logging::FieldPatch::Set(share::ids::new_typed_id("irq")),
        model: logging::FieldPatch::Set(model.to_string()),
        provider: logging::FieldPatch::Set(provider.to_string()),
        role: logging::FieldPatch::Set(role.to_string()),
        ..logging::LogContextPatch::default()
    })
}

#[cfg(test)]
fn loop_input_messages(inputs: &[crate::application::loop_engine::LoopInput]) -> Vec<Message> {
    inputs.iter().map(|input| input.message()).collect()
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
    agent_runner: Option<Arc<dyn tools::published::agent::AgentRunner>>,
    language: &str,
    workspace: &project::Workspace,
    cancel: &CancellationToken,
    read_files: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    max_tool_concurrency: usize,
    agent_semaphore: Arc<tokio::sync::Semaphore>,
    session_id: &str,
    background_processes: Option<
        std::sync::Arc<
            crate::application::background_process::session_runtime::BackgroundProcessRuntime,
        >,
    >,
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
        session_id: context::SessionId::new(session_id),
        ctx: tools::published::execution::ToolExecutionContext::new(
            tools::ExecutionScope::builder(
                run_id.to_string(),
                workspace.read().workspace_id(),
                workspace.read().current_workspace_root(),
            )
            .build(),
            tools::published::execution::ToolExecutionPorts::new(
                Arc::new(runtime_context.cancel().clone()),
                crate::application::run::workspace::RuntimeWorkspaceAccess::new(workspace.clone())
                    .read_access(),
                Arc::new(tools::MutexReadSet(read_files)),
                runtime_context.memory(),
                Arc::new(tools::FixedGuidance {
                    language: language.to_string(),
                }),
            )
            .with_user_agent(&runtime_provider.user_agent)
            .with_memory_context(Some(session_id.to_string()))
            .with_skill_load_state(
                tools::published::skill::SkillLoadScope::main(),
                runtime_context.skill_load_state(),
            )
            .with_scoring(
                // #1835：skill_match 开关开启且评分端口装配时注入 ToolSearch 语义重排
                //（读 skill_match 专属槽位，与 memory recall 槽互不串用）。
                runtime_context
                    .scoring_for_skill_match()
                    .filter(|_| runtime_context.config_ref().config().scoring().skill_match),
            )
            .with_agent(agent_runner),
        ),
        max_tool_concurrency,
        agent_semaphore,
        workspace_persist: workspace.persist(),
        tool_result_materializer,
        committed_side_effects:
            crate::application::loop_engine::chat::committed_side_effect::task_dispatcher(
                runtime_context,
                session_id.to_string(),
                workspace.read().current_workspace_root(),
            ),
        runtime_cancellation: cancel.clone(),
        background_threshold: crate::application::run::config::background_threshold_from_context(
            runtime_context,
        ),
        background_processes,
    }
}

pub(crate) struct ChatEventPort {
    pub sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
    pub session_id: String,
    pub turn_context: RuntimeRunContext,
    pub task_access: Arc<dyn task::TaskAccess>,
    pub model: String,
}

#[async_trait]
impl EventSinkPort for ChatEventPort {
    async fn emit(
        &mut self,
        execution: &mut RunExecutionState,
        events: Vec<RuntimeLifecycleEvent>,
    ) -> Result<(), LoopEngineError> {
        ChatStreamEventObserver {
            sink: self.sink.clone(),
            session_id: &self.session_id,
            turn_context: &self.turn_context,
            task_access: &self.task_access,
            model: &self.model,
            started_at: execution.started_at().unwrap_or_else(Instant::now),
            step_count: execution.run_ordinal(),
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

/// 自动压缩观察者：不在回调内执行反思（回调拿不到 `&mut Run`，无法驱动状态机），
/// 仅在 `Committed` 时把将被丢弃的消息暂存进与反思端口共享的材料槽，由 engine 的
/// reflection phase 在 Compacting 内取出执行；`Skipped` 不动槽位。
pub(crate) struct ChatCompactionObserver {
    pub pre_compact_material:
        crate::application::loop_engine::chat::reflection::PreCompactMaterialSlot,
}

#[async_trait]
impl crate::application::loop_engine::compaction::CompactionObserver for ChatCompactionObserver {
    async fn on_compacted(
        &mut self,
        outcome: &crate::ports::CompactOutcome,
        discarded_messages: &[Message],
    ) -> Result<(), LoopEngineError> {
        if matches!(outcome, crate::ports::CompactOutcome::Committed(_)) {
            self.pre_compact_material.stage(discarded_messages.to_vec());
        }
        Ok(())
    }
}

/// idle `/compact` 的手动压缩端口：由会话驱动装配，承载会话级入参（system prompt、
/// context size、task snapshot）并发布用户可见结果。
pub(crate) struct ChatManualCompaction {
    pub runtime_context: RuntimeContext,
    pub session_id: String,
    pub system_prompt: String,
    pub context_size: usize,
}

#[async_trait]
impl crate::application::loop_engine::ManualCompactionPort for ChatManualCompaction {
    async fn manual_compact(
        &mut self,
        run_id: &sdk::RunId,
        _cancel: &CancellationToken,
        progress: Arc<dyn crate::application::loop_engine::CompactProgressView>,
    ) -> Result<crate::application::loop_engine::ManualCompactionOutcome, LoopEngineError> {
        let coordinator = crate::application::context::coordination::ContextCoordinator::new(
            self.runtime_context.context(),
        );
        let task_snapshot =
            crate::application::loop_engine::chat::task_snapshot::build_compact_task_snapshot(
                self.runtime_context.task().as_ref(),
            );
        let progress_callback: Arc<dyn context::compact::CompactProgressFn> =
            Arc::new(move |stage, work| {
                progress.emit(compact_stage_view(stage), compact_work_view(work));
            });
        let request = crate::ports::ManualCompactRequestData {
            session_id: crate::ports::SessionId::new(self.session_id.clone()),
            run_id: run_id.clone(),
            system_prompt: crate::ports::SystemPromptSpecData::new(self.system_prompt.clone()),
            context_size: self.context_size,
            progress: Some(progress_callback),
            task_snapshot,
        };
        match coordinator.manual_compact(&request).await {
            Ok(crate::ports::CompactOutcome::Committed(result)) => {
                self.runtime_context
                    .event_sink()
                    .send_event(RuntimeStreamEvent::CompactOperationCompleted {
                        messages: result.recent_messages,
                        notice: "✓ 上下文压缩完成".to_string(),
                    })
                    .await;
                Ok(crate::application::loop_engine::ManualCompactionOutcome::Committed)
            }
            Ok(crate::ports::CompactOutcome::Skipped(_)) => {
                self.runtime_context
                    .event_sink()
                    .send_event(RuntimeStreamEvent::SystemMessage(
                        "Not enough messages to compact.".to_string(),
                    ))
                    .await;
                Ok(crate::application::loop_engine::ManualCompactionOutcome::Skipped)
            }
            Err(error) => Err(LoopEngineError::Adapter(format!(
                "Session compact 失败：{error}"
            ))),
        }
    }
}

/// idle `/reflect-now` 的手动反思端口：由会话驱动装配，承载装配前冻结的 committed
/// 会话消息快照，执行一次反思并按 `manual_reflection_outcome_text` 发布终态文案。
/// 状态机与 `Reflection` activity 由 engine 的 `execute_manual_reflection` 持有。
pub(crate) struct ChatManualReflection {
    pub runtime_context: RuntimeContext,
    pub reflection_tasks: crate::application::reflection::ReflectionTaskAdapter,
    pub system_prompt: String,
    pub language: String,
    pub messages: Vec<Message>,
    /// 反思游标推进基准（#1827）：装配时快照的 session 历史总长；Succeeded 落盘写入。
    pub coverage_end: Option<u64>,
}

#[async_trait]
impl crate::application::loop_engine::ManualReflectionPort for ChatManualReflection {
    async fn run_manual_reflection(
        &mut self,
        run_id: &sdk::RunId,
        cancel: &CancellationToken,
    ) -> Result<crate::application::loop_engine::ManualReflectionOutcome, LoopEngineError> {
        let outcome = crate::application::loop_engine::chat::reflection::run(
            &self.reflection_tasks,
            crate::application::reflection::ReflectionTaskTrigger::Manual,
            self.runtime_context.config_ref().config().memory(),
            std::mem::take(&mut self.messages),
            self.runtime_context.provider_ref(),
            &self.system_prompt,
            &self.language,
            self.runtime_context.memory_ref(),
            self.runtime_context.reflection_history_ref(),
            self.coverage_end,
            cancel.clone(),
        )
        .await;
        // 成功手动反思复用 `RuntimeReflection` 同一记账路径
        // （共享 `record_successful_usage`）；Manual Run 无 RunStep，
        // `run_step_id=None` → 仅记账 UUIDv7。Failed/Cancelled/TimedOut/
        // DisabledSkipped 不在此分支，不记账。
        if let crate::application::reflection::ReflectionRunOutcome::Completed(completion) =
            &outcome
        {
            if completion.status
                == crate::application::reflection::ReflectionTaskCompletionStatus::Succeeded
            {
                if let Some(metadata) = &completion.metadata {
                    crate::application::loop_engine::run_services::RuntimeReflection::record_succeeded_usage(
                        &self.runtime_context,
                        run_id,
                        None,
                        metadata,
                    );
                }
            }
        }
        let (text, is_error) =
            crate::application::loop_engine::chat::reflection::manual_reflection_outcome_text(
                &outcome,
            );
        self.runtime_context
            .event_sink()
            .send_event(RuntimeStreamEvent::CommandResultText { text, is_error })
            .await;
        Ok(match outcome {
            crate::application::reflection::ReflectionRunOutcome::Completed(completion) => {
                match completion.status {
                    crate::application::reflection::ReflectionTaskCompletionStatus::Cancelled => {
                        crate::application::loop_engine::ManualReflectionOutcome::Cancelled
                    }
                    crate::application::reflection::ReflectionTaskCompletionStatus::TimedOut => {
                        crate::application::loop_engine::ManualReflectionOutcome::TimedOut
                    }
                    status => {
                        crate::application::loop_engine::ManualReflectionOutcome::Ready(status)
                    }
                }
            }
            crate::application::reflection::ReflectionRunOutcome::DisabledSkipped => {
                // 受理门禁已在 idle 时判定过；这里只可能是配置在受理后被关闭的竞态
                // （文案已按「未启用」发布，Run 照常收口，activity 记 Failed）。
                log::warn!(
                    target: crate::LOG_TARGET,
                    "[manual_reflection] 反思配置在受理后被关闭，按失败收口 run_id={run_id}"
                );
                crate::application::loop_engine::ManualReflectionOutcome::Ready(
                    crate::application::reflection::ReflectionTaskCompletionStatus::Failed,
                )
            }
        })
    }
}

fn compact_stage_view(stage: context::compact::CompactStageData) -> sdk::CompactStageView {
    match stage {
        context::compact::CompactStageData::Preparing => sdk::CompactStageView::Preparing,
        context::compact::CompactStageData::Generating => sdk::CompactStageView::Generating,
        context::compact::CompactStageData::Mapping => sdk::CompactStageView::Mapping,
        context::compact::CompactStageData::Reducing => sdk::CompactStageView::Reducing,
        context::compact::CompactStageData::Refreshing => sdk::CompactStageView::Refreshing,
        context::compact::CompactStageData::Finalizing => sdk::CompactStageView::Finalizing,
    }
}

fn compact_work_view(work: context::compact::CompactWorkData) -> sdk::CompactWorkView {
    match work {
        context::compact::CompactWorkData::Indeterminate => sdk::CompactWorkView::Indeterminate,
        context::compact::CompactWorkData::Determinate { completed, total } => {
            let (Ok(completed), Ok(total)) = (u32::try_from(completed), u32::try_from(total))
            else {
                return sdk::CompactWorkView::Indeterminate;
            };
            sdk::CompactWorkView::Determinate { completed, total }
        }
    }
}

pub(crate) struct ChatStopHookObserver {
    pub sink: crate::application::loop_engine::chat::ChatEventSinkHandle,
    pub continuation: InputContinuationState,
}

#[async_trait]
impl crate::application::hook::stop_coordination::StopHookObserver for ChatStopHookObserver {
    fn install_stop_hook_feedback(&mut self, message: Message) {
        self.continuation.install_stop_hook_feedback(message);
    }

    async fn observe_stop_hook_outcome(
        &mut self,
        execution: &RunExecutionState,
        outcome: &crate::application::hook::stop_coordination::StopHookOutcome,
    ) -> Result<(), LoopEngineError> {
        if let Some(message) = outcome.feedback_message.as_ref() {
            let notice = message
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.hook_notice.clone())
                .expect("Stop Hook feedback message must carry typed Hook notice");
            self.sink
                .send_event(RuntimeStreamEvent::HookNotice(notice))
                .await;
            self.sink
                .send_message_state_changed(execution.messages_len())
                .await;
        }
        Ok(())
    }
}

pub(crate) struct ChatToolRoundObserver {
    pub runtime_context: RuntimeContext,
    pub workspace_read: Arc<dyn project::WorkspaceReader>,
    pub turn_context: RuntimeRunContext,
    pub session_id: String,
    pub materializer:
        Arc<crate::application::tool::tool_result_materializer::ToolResultMaterializer>,
}

#[async_trait]
impl crate::application::tool::coordination::ToolRoundObserver for ChatToolRoundObserver {
    async fn cancelled_results_completed(
        &mut self,
        results: &[crate::application::tool::agent::ToolExecution],
    ) {
        for result in results {
            crate::application::loop_engine::chat::tools::send_tool_result(
                &self.runtime_context.event_sink(),
                &self.turn_context,
                result,
                self.materializer.as_ref(),
                &self.session_id,
            )
            .await;
        }
    }

    async fn results_materialized(&mut self, execution: &RunExecutionState) {
        self.runtime_context
            .event_sink()
            .send_message_state_changed(execution.messages_len())
            .await;
    }

    async fn round_finished(
        &mut self,
        step_id: &sdk::RunStepId,
        call_count: usize,
        run_step: usize,
        cancel: &CancellationToken,
    ) {
        run_post_tool_batch(
            self.runtime_context.hooks_ref(),
            self.runtime_context.activities().as_ref(),
            step_id,
            self.runtime_context.main_session_id(),
            cancel,
            call_count,
            run_step,
            &self.workspace_read,
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
    pub context_size: usize,
    pub turn_context: RuntimeRunContext,
    pub tool_identity: crate::application::tool::coordination::identity::ToolIdentityRegistry,
    /// #1494：边流边执行句柄（流中 ToolCallCompleted → 立即执行，结果缓冲）。
    pub streaming_tool:
        Option<Arc<crate::application::loop_engine::chat::streaming_tool::StreamingToolExecutor>>,
}

impl<I> ChatModelObserver<I>
where
    I: SessionInputPort,
{
    async fn queue_busy_event(&mut self, event: sdk::ChatInputEvent) {
        match event {
            sdk::ChatInputEvent::UserMessage { .. } => self.input.admit_user_message(event).await,
            sdk::ChatInputEvent::WithdrawAll => {
                // 撤回语义覆盖所有待处理输入：Run 内消息 + 排队的控制命令（#1816）。
                let mut texts = self
                    .input
                    .run_input_buffer
                    .with_lock(|buffer| buffer.withdraw_all_user_texts());
                let withdrawn_commands = self.input.pending_input.drain_for_withdraw();
                if !withdrawn_commands.is_empty() {
                    texts.extend(withdrawn_commands);
                    self.runtime_context
                        .event_sink()
                        .send_event(RuntimeStreamEvent::ControlCommandsQueued {
                            queued: self.input.pending_input.command_snapshot(),
                        })
                        .await;
                }
                self.runtime_context
                    .event_sink()
                    .send_event(RuntimeStreamEvent::UserMessagesWithdrawn { texts })
                    .await;
            }
            other => {
                // busy 期间的控制类命令入队：发布全量快照供 UI 回显（#1816）。
                self.input.pending_input.push(other);
                self.runtime_context
                    .event_sink()
                    .send_event(RuntimeStreamEvent::ControlCommandsQueued {
                        queued: self.input.pending_input.command_snapshot(),
                    })
                    .await;
            }
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
        let reducer = InvocationEventReducer::with_tool_identity(
            self.runtime_context.event_sink(),
            self.tool_identity.clone(),
            self.turn_context.clone(),
        );
        match &self.streaming_tool {
            Some(executor) => reducer.with_streaming_tool(executor.clone()),
            None => reducer,
        }
    }

    fn streaming_tool(
        &self,
    ) -> Option<&Arc<crate::application::loop_engine::chat::streaming_tool::StreamingToolExecutor>>
    {
        self.streaming_tool.as_ref()
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
        // #1818：一次 drain 就是一批，同批连续用户消息折叠为一条再入队。
        for event in crate::application::loop_engine::batched_user_input::fold_batched_user_inputs(
            self.input.input_events.drain_input_events().await,
        ) {
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
        _execution: &mut RunExecutionState,
        response: &crate::application::loop_engine::chat::InvocationResponse,
        calls: Vec<ToolCall>,
        usage: crate::application::loop_engine::StepTokenUsage,
        _cancel: &CancellationToken,
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
        // Interval 反思的判定与执行在 engine reflection phase
        // （状态机只在 engine 可达），本函数只做纯终态分类。
        Ok((
            ModelStep::Complete {
                text: response.assistant_message.text_content(),
            },
            usage,
        ))
    }
}

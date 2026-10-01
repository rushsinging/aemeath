use std::sync::Arc;

use async_trait::async_trait;
use share::message::Message;
use tokio_util::sync::CancellationToken;

use crate::application::hook::stop_coordination::{
    StopHookExecutionContext, StopHookObserver, StopHookOutcome,
};
use crate::application::interaction::coordinator::{
    InteractionCompletionContext, InteractionCompletionContextProvider,
};
use crate::application::interaction::port::InteractionPort;
use crate::application::loop_engine::chat::events::ChatEventSink as _;
use crate::application::loop_engine::chat::RuntimeStreamEvent;
use crate::application::loop_engine::compaction::{CompactionCoordinator, CompactionObserver};
use crate::application::loop_engine::context_request::{
    ContextRequestCoordinator, ContextRequestSource,
};
use crate::application::loop_engine::step_persistence::{
    AcceptedInputObserver, StepPersistenceCoordinator,
};
use crate::application::loop_engine::{
    CompactProgressView, CompactionPort, InteractionMailboxPort, LoopEngineError,
    ModelInvocationPort, PendingInteractionWork, ReflectionPhasePort, StepCommit,
    StepPersistencePort, ToolGuardDecision, ToolOrchestrationPort,
};
use crate::application::reflection::{
    ReflectionRunOutcome, ReflectionTaskAdapter, ReflectionTaskCompletionStatus,
};
use crate::application::run::context::RuntimeContext;
use crate::application::run::execution_state::RunExecutionState;
use crate::application::tool::tool_result_materializer::ToolResultMaterializer;
use crate::ports::{ContextRequestData as _CtxRequestData, RunStepId};

pub(crate) struct ContextRequest<'a> {
    pub runtime_context: &'a RuntimeContext,
    pub session_id: &'a str,
    pub system_prompt: &'a str,
    pub model_id: &'a str,
    pub language: &'a str,
    pub agent_roles: std::collections::HashMap<String, share::config::AgentRoleDefinition>,
    pub config: &'a crate::application::run::config::RunConfigSnapshot,
    pub context_size: usize,
    pub max_output_tokens: usize,
    pub raw_tool_schemas: Vec<serde_json::Value>,
    pub invocation_reminders: Vec<context::InvocationReminderData>,
}

pub(crate) struct RuntimeStepPersistence<'a, O> {
    run_id: sdk::RunId,
    context_request: ContextRequest<'a>,
    input_prefix: Option<Message>,
    accepted_input: O,
    reminder_intents_available: bool,
}

impl<'a, O> RuntimeStepPersistence<'a, O>
where
    O: AcceptedInputObserver,
{
    pub(crate) fn new(
        run_id: sdk::RunId,
        context_request: ContextRequest<'a>,
        input_prefix: Option<Message>,
        accepted_input: O,
    ) -> Self {
        Self {
            run_id,
            context_request,
            input_prefix,
            accepted_input,
            reminder_intents_available: true,
        }
    }

    fn source(&self) -> ContextRequestSource<'_> {
        ContextRequestSource {
            runtime_context: self.context_request.runtime_context,
            session_id: self.context_request.session_id,
            system_prompt: self.context_request.system_prompt,
            model_id: self.context_request.model_id,
            language: self.context_request.language,
            agent_roles: self.context_request.agent_roles.clone(),
            config: self.context_request.config,
            context_size: self.context_request.context_size,
            max_output_tokens: self.context_request.max_output_tokens,
            raw_tool_schemas: self.context_request.raw_tool_schemas.clone(),
        }
    }
}

#[async_trait]
impl<O> StepPersistencePort for RuntimeStepPersistence<'_, O>
where
    O: AcceptedInputObserver,
{
    fn take_step_input_prefix(&mut self) -> Option<Message> {
        self.input_prefix.take()
    }

    fn build_context_request(
        &self,
        execution: &RunExecutionState,
        _run_id: &sdk::RunId,
        step_id: &RunStepId,
    ) -> Option<_CtxRequestData> {
        let mut request = ContextRequestCoordinator::new(self.source()).build_request(
            &self.run_id,
            step_id,
            execution.step_outcome(),
        );
        if self.reminder_intents_available {
            request.invocation_reminders = self.context_request.invocation_reminders.clone();
            if !request.invocation_reminders.is_empty() {
                let kinds = request
                    .invocation_reminders
                    .iter()
                    .map(context::InvocationReminderData::kind)
                    .collect::<Vec<_>>()
                    .join(",");
                log::debug!(
                    target: crate::LOG_TARGET,
                    "invocation_reminders_attached count={} kinds={} run_id={} step_id={}",
                    request.invocation_reminders.len(),
                    kinds,
                    self.run_id,
                    step_id.as_str(),
                );
            }
        } else if !self.context_request.invocation_reminders.is_empty() {
            log::debug!(
                target: crate::LOG_TARGET,
                "invocation_reminders_skipped reason=already_consumed count={} run_id={} step_id={}",
                self.context_request.invocation_reminders.len(),
                self.run_id,
                step_id.as_str(),
            );
        }
        Some(request)
    }

    async fn accept_step_input(
        &mut self,
        execution: &mut RunExecutionState,
        step_id: &RunStepId,
    ) -> Result<(), LoopEngineError> {
        self.reminder_intents_available = false;
        StepPersistenceCoordinator::from_context(self.context_request.runtime_context)
            .accept_step_input(execution, step_id, &mut self.accepted_input)
            .await
    }

    async fn load_step_receipts(
        &mut self,
        request: &crate::ports::ContextRequestData,
    ) -> Result<Vec<crate::ports::StepReceiptData>, LoopEngineError> {
        StepPersistenceCoordinator::from_context(self.context_request.runtime_context)
            .load_step_receipts(request)
            .await
    }

    async fn persist_step_commit(&mut self, commit: &StepCommit) -> Result<(), LoopEngineError> {
        StepPersistenceCoordinator::from_context(self.context_request.runtime_context)
            .persist_step_commit(commit)
            .await
    }
}

pub(crate) struct RuntimeCompaction<'a, O> {
    runtime_context: &'a RuntimeContext,
    observer: O,
}

impl<'a, O> RuntimeCompaction<'a, O>
where
    O: CompactionObserver,
{
    pub(crate) fn new(runtime_context: &'a RuntimeContext, observer: O) -> Self {
        Self {
            runtime_context,
            observer,
        }
    }
}

#[async_trait]
impl<O> CompactionPort for RuntimeCompaction<'_, O>
where
    O: CompactionObserver,
{
    async fn needs_compaction(
        &mut self,
        execution: &mut RunExecutionState,
    ) -> Result<bool, LoopEngineError> {
        CompactionCoordinator::from_context(self.runtime_context)
            .needs_compaction(execution)
            .await
    }

    async fn compact(
        &mut self,
        execution: &mut RunExecutionState,
        cancel: &CancellationToken,
        progress: std::sync::Arc<dyn CompactProgressView>,
    ) -> Result<(), LoopEngineError> {
        let task_snapshot =
            crate::application::loop_engine::chat::task_snapshot::build_compact_task_snapshot(
                &*self.runtime_context.task_ref().clone(),
            );
        CompactionCoordinator::from_context(self.runtime_context)
            .compact(
                execution,
                &mut self.observer,
                progress,
                task_snapshot,
                cancel.clone(),
            )
            .await
    }
}

/// 生产反思端口：判定材料与反思执行的唯一装配点（状态机与
/// activity 由 engine phase 持有，本端口只回答「有没有要做的事」并执行反思）。
/// 持有与压缩观察者共享的 PreCompact 材料槽：观察者在 Committed 时暂存，本端口
/// 在 engine 的 Compacting 相位内取出（反思禁用时取出即丢弃，不滞留不空走往返）。
pub(crate) struct RuntimeReflection<'a> {
    runtime_context: &'a RuntimeContext,
    reflection_tasks: ReflectionTaskAdapter,
    system_prompt: String,
    language: String,
    pre_compact_material: crate::application::loop_engine::chat::reflection::PreCompactMaterialSlot,
}

impl<'a> RuntimeReflection<'a> {
    pub(crate) fn new(
        runtime_context: &'a RuntimeContext,
        reflection_tasks: ReflectionTaskAdapter,
        system_prompt: String,
        language: String,
        pre_compact_material: crate::application::loop_engine::chat::reflection::PreCompactMaterialSlot,
    ) -> Self {
        Self {
            runtime_context,
            reflection_tasks,
            system_prompt,
            language,
            pre_compact_material,
        }
    }

    /// 反思成功 terminal 的 Usage 记账唯一实现。
    ///
    /// Interval/PreCompact（本端口 `run_reflection`）与 Manual
    /// （`ChatManualReflection`）共用本函数，经共享
    /// [`crate::application::model::invocation::record_successful_usage`]
    /// 恰好落 1 条 `UsageRecordData`；Failed/Cancelled/TimedOut/DisabledSkipped
    /// 不调用故不记账，retry/fallback 发生在执行层内部，只有最终
    /// `Succeeded` 终态会经过这里（不重复记账）。
    ///
    /// Manual Reflection Run 无 RunStep：`run_step_id=None` 时生成仅记账用的
    /// UUIDv7；`model_invocation_id` 同理。不发布 cost。
    pub(crate) fn record_succeeded_usage(
        runtime_context: &RuntimeContext,
        run_id: &sdk::RunId,
        run_step_id: Option<&sdk::RunStepId>,
        metadata: &crate::application::reflection::ReflectionTaskMetadata,
    ) {
        let usage = crate::ports::RawUsageSnapshotData {
            input_tokens: Some(metadata.input_tokens),
            output_tokens: Some(metadata.output_tokens),
            ..crate::ports::RawUsageSnapshotData::default()
        };
        crate::application::model::invocation::record_successful_usage(
            runtime_context.usage_sink().as_ref(),
            crate::application::model::usage::UsageRecordContext {
                session_id: sdk::SessionId::new(runtime_context.skill_load_session_id()),
                run_id: run_id.clone(),
                // Manual Reflection Run 无 RunStep：仅记账 id（PR 披露）。
                run_step_id: run_step_id.cloned().unwrap_or_else(sdk::RunStepId::new_v7),
                model_invocation_id: sdk::ModelInvocationId::new_v7(),
                model: runtime_context.provider_ref().model.clone(),
            },
            usage,
            crate::application::model::invocation::unix_timestamp_millis,
        );
    }
}

#[async_trait]
impl ReflectionPhasePort for RuntimeReflection<'_> {
    fn interval_reflection_messages(
        &self,
        step_count: usize,
        messages: &[Message],
    ) -> Option<Vec<Message>> {
        let memory_config = self.runtime_context.config_ref().config().memory();
        if crate::application::loop_engine::chat::reflection::should_run_turn_reflection(
            memory_config,
            step_count,
        ) {
            Some(messages.to_vec())
        } else {
            None
        }
    }

    fn take_pre_compact_messages(&self) -> Option<Vec<Message>> {
        self.pre_compact_material
            .take_for_reflection(self.runtime_context.config_ref().config().memory())
    }

    async fn run_reflection(
        &mut self,
        trigger: crate::application::reflection::ReflectionTaskTrigger,
        messages: Vec<Message>,
        run_id: &sdk::RunId,
        run_step_id: Option<&sdk::RunStepId>,
        cancel: CancellationToken,
    ) -> Result<ReflectionRunOutcome, LoopEngineError> {
        let outcome = crate::application::loop_engine::chat::reflection::run(
            &self.reflection_tasks,
            trigger,
            self.runtime_context.config_ref().config().memory(),
            messages,
            self.runtime_context.provider_ref(),
            &self.system_prompt,
            &self.language,
            self.runtime_context.memory_ref(),
            self.runtime_context.reflection_history_ref(),
            cancel,
        )
        .await;
        // 仅 Succeeded 且带 usage metadata 的终态经共享
        // `record_successful_usage` 计入 /usage（Interval/PreCompact/Manual 同路径）。
        if let ReflectionRunOutcome::Completed(completion) = &outcome {
            if completion.status == ReflectionTaskCompletionStatus::Succeeded {
                if let Some(metadata) = &completion.metadata {
                    Self::record_succeeded_usage(
                        self.runtime_context,
                        run_id,
                        run_step_id,
                        metadata,
                    );
                }
            }
        }
        // 行为从 classify_terminal 原样搬迁：成功且有记忆变更时发布 TUI notice。
        crate::application::loop_engine::chat::reflection::announce_memory_update(
            &self.runtime_context.event_sink(),
            &outcome,
            &self.language,
        )
        .await;
        Ok(outcome)
    }
}

#[async_trait]
pub(crate) trait InteractionPublisher: Send {
    fn interaction_port(&self) -> &dyn InteractionPort;
    fn completion_context(
        &self,
        step_cancel: CancellationToken,
    ) -> InteractionCompletionContext<'_>;
    async fn publish(
        &mut self,
        execution: &RunExecutionState,
        request: &sdk::InteractionRequest,
    ) -> Result<(), LoopEngineError>;
}

pub(crate) struct RuntimeInteraction<P> {
    publisher: P,
}

impl<P> RuntimeInteraction<P>
where
    P: InteractionPublisher,
{
    pub(crate) fn new(publisher: P) -> Self {
        Self { publisher }
    }
}

impl<P> InteractionCompletionContextProvider for RuntimeInteraction<P>
where
    P: InteractionPublisher,
{
    fn interaction_completion_context(
        &self,
        step_cancel: CancellationToken,
    ) -> InteractionCompletionContext<'_> {
        self.publisher.completion_context(step_cancel)
    }
}

#[async_trait]
impl<P> InteractionMailboxPort for RuntimeInteraction<P>
where
    P: InteractionPublisher,
{
    fn interaction_port(&self) -> &dyn InteractionPort {
        self.publisher.interaction_port()
    }

    async fn publish_interaction(
        &mut self,
        execution: &RunExecutionState,
        request: &sdk::InteractionRequest,
    ) -> Result<(), LoopEngineError> {
        self.publisher.publish(execution, request).await
    }

    fn set_pending_interaction_work(
        &mut self,
        execution: &mut RunExecutionState,
        work: PendingInteractionWork,
    ) {
        execution.set_pending_interaction_work(work);
    }
}

pub(crate) struct ChatInteractionPublisher<'a> {
    pub runtime_context: &'a RuntimeContext,
    pub tool_context: tools::published::execution::ToolExecutionContext,
    pub materializer: &'a ToolResultMaterializer,
    pub session_id: &'a str,
}

#[async_trait]
impl InteractionPublisher for ChatInteractionPublisher<'_> {
    fn interaction_port(&self) -> &dyn InteractionPort {
        self.runtime_context.interaction_ref().as_ref()
    }

    fn completion_context(
        &self,
        step_cancel: CancellationToken,
    ) -> InteractionCompletionContext<'_> {
        InteractionCompletionContext::new(
            self.tool_context.with_cancellation(Arc::new(
                crate::application::run::context::RunCancellationScope::from_token(step_cancel),
            )),
            self.runtime_context.tool_execution_ref().as_ref(),
            self.materializer,
            self.session_id,
        )
    }

    async fn publish(
        &mut self,
        _execution: &RunExecutionState,
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
}

pub(crate) struct ProgressInteractionPublisher<'a> {
    pub runtime_context: &'a RuntimeContext,
    pub tool_context: tools::published::execution::ToolExecutionContext,
    pub session_id: &'a str,
    pub materializer: &'a ToolResultMaterializer,
    pub progress: &'a (dyn Fn(Option<usize>, &str) + Send + Sync),
}

#[async_trait]
impl InteractionPublisher for ProgressInteractionPublisher<'_> {
    fn interaction_port(&self) -> &dyn InteractionPort {
        self.runtime_context.interaction_ref().as_ref()
    }

    fn completion_context(
        &self,
        step_cancel: CancellationToken,
    ) -> InteractionCompletionContext<'_> {
        InteractionCompletionContext::new(
            self.tool_context.with_cancellation(Arc::new(
                crate::application::run::context::RunCancellationScope::from_token(step_cancel),
            )),
            self.runtime_context.tool_execution_ref().as_ref(),
            self.materializer,
            self.session_id,
        )
    }

    async fn publish(
        &mut self,
        execution: &RunExecutionState,
        request: &sdk::InteractionRequest,
    ) -> Result<(), LoopEngineError> {
        (self.progress)(
            Some(execution.step_count()),
            &format!("Interaction: id={}", request.id),
        );
        Ok(())
    }
}

pub(crate) struct RuntimeModelInvocation<O> {
    observer: O,
    advance_step: bool,
}

impl<O> RuntimeModelInvocation<O>
where
    O: crate::application::model::invocation::ModelInvocationObserver,
{
    pub(crate) fn new(observer: O, advance_step: bool) -> Self {
        Self {
            observer,
            advance_step,
        }
    }
}

#[async_trait]
impl<O> ModelInvocationPort for RuntimeModelInvocation<O>
where
    O: crate::application::model::invocation::ModelInvocationObserver,
{
    async fn invoke_model(
        &mut self,
        execution: &mut RunExecutionState,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        invocation_id: &sdk::ModelInvocationId,
        cancel: &CancellationToken,
    ) -> Result<
        (
            crate::application::loop_engine::ModelStep,
            crate::application::loop_engine::StepTokenUsage,
        ),
        LoopEngineError,
    > {
        if self.advance_step {
            execution.advance_step();
        }
        let run_step = execution.step_count();
        logging::within(
            logging::LogContextPatch {
                run_step: logging::FieldPatch::Set(run_step),
                request_id: logging::FieldPatch::Clear,
                ..logging::LogContextPatch::default()
            },
            crate::application::model::invocation::orchestrate_model_invocation(
                &mut self.observer,
                execution,
                run_id,
                step_id,
                invocation_id,
                cancel,
            ),
        )
        .await
    }

    async fn take_streaming_tool_results(
        &mut self,
    ) -> Vec<crate::application::loop_engine::chat::streaming_tool::StreamingToolRoundResult> {
        match self.observer.streaming_tool() {
            Some(executor) => executor.take_results().await,
            None => Vec::new(),
        }
    }
}

pub(crate) struct RuntimeToolOrchestration<'a, O> {
    coordinator: crate::application::tool::coordination::ToolRoundCoordinator<'a, O>,
}

impl<'a, O> RuntimeToolOrchestration<'a, O>
where
    O: crate::application::tool::coordination::ToolRoundObserver,
{
    pub(crate) fn new(
        context: crate::application::tool::coordination::ToolRoundContext<'a>,
        observer: O,
    ) -> Self {
        Self {
            coordinator: crate::application::tool::coordination::ToolRoundCoordinator::new(
                context, observer,
            ),
        }
    }
}

#[async_trait]
impl<O> ToolOrchestrationPort for RuntimeToolOrchestration<'_, O>
where
    O: crate::application::tool::coordination::ToolRoundObserver,
{
    async fn execute_tools(
        &mut self,
        execution: &mut RunExecutionState,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        calls: &[(crate::application::tool::agent::ToolCall, ToolGuardDecision)],
        cancel: &CancellationToken,
    ) -> Result<crate::application::tool::coordination::ToolRoundOutcome, LoopEngineError> {
        self.coordinator
            .execute(execution, run_id, step_id, calls, cancel)
            .await
    }

    async fn finalize_streaming_tool_results(
        &mut self,
        execution: &mut RunExecutionState,
        step_id: &sdk::RunStepId,
        rounds: Vec<
            crate::application::loop_engine::chat::streaming_tool::StreamingToolRoundResult,
        >,
        cancel: &CancellationToken,
    ) -> Result<crate::application::tool::coordination::ToolRoundOutcome, LoopEngineError> {
        self.coordinator
            .finalize_streaming(execution, step_id, rounds, cancel)
            .await
    }
}

pub(crate) struct RuntimeStopHook<O> {
    context: StopHookExecutionContext,
    observer: O,
}

impl<O> RuntimeStopHook<O> {
    pub(crate) fn new(context: StopHookExecutionContext, observer: O) -> Self {
        Self { context, observer }
    }
}

#[async_trait]
impl<O> StopHookObserver for RuntimeStopHook<O>
where
    O: StopHookObserver,
{
    fn stop_hook_execution_context(&self) -> Option<StopHookExecutionContext> {
        Some(self.context.clone())
    }

    fn install_stop_hook_feedback(&mut self, message: Message) {
        self.observer.install_stop_hook_feedback(message);
    }

    async fn observe_stop_hook_outcome(
        &mut self,
        execution: &RunExecutionState,
        outcome: &StopHookOutcome,
    ) -> Result<(), LoopEngineError> {
        self.observer
            .observe_stop_hook_outcome(execution, outcome)
            .await
    }
}

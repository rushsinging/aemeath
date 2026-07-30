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
    CompactionPort, InteractionMailboxPort, LoopEngineError, ModelInvocationPort,
    PendingInteractionWork, StepCommit, StepPersistencePort, ToolGuardDecision,
    ToolOrchestrationPort,
};
use crate::application::run::context::RuntimeContext;
use crate::application::run::execution_state::RunExecutionState;
use crate::application::tool::result_materialization::ToolResultMaterializer;
use crate::ports::{ContextRequest, RunStepId};

pub(crate) struct ContextRequestData<'a> {
    pub runtime_context: &'a RuntimeContext,
    pub session_id: &'a str,
    pub system_prompt: &'a str,
    pub model_id: &'a str,
    pub language: &'a str,
    pub agent_roles: std::collections::HashMap<String, share::config::AgentRoleConfig>,
    pub config: &'a crate::application::run::config::RunConfigSnapshot,
    pub context_size: usize,
    pub max_output_tokens: usize,
    pub raw_tool_schemas: Vec<serde_json::Value>,
}

pub(crate) struct RuntimeStepPersistence<'a, O> {
    run_id: sdk::RunId,
    context_request: ContextRequestData<'a>,
    input_prefix: Option<Message>,
    accepted_input: O,
}

impl<'a, O> RuntimeStepPersistence<'a, O>
where
    O: AcceptedInputObserver,
{
    pub(crate) fn new(
        run_id: sdk::RunId,
        context_request: ContextRequestData<'a>,
        input_prefix: Option<Message>,
        accepted_input: O,
    ) -> Self {
        Self {
            run_id,
            context_request,
            input_prefix,
            accepted_input,
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
    ) -> Option<ContextRequest> {
        Some(ContextRequestCoordinator::new(self.source()).build_request(
            &self.run_id,
            step_id,
            execution.step_outcome(),
        ))
    }

    async fn accept_step_input(
        &mut self,
        execution: &mut RunExecutionState,
        step_id: &RunStepId,
    ) -> Result<(), LoopEngineError> {
        StepPersistenceCoordinator::from_context(self.context_request.runtime_context)
            .accept_step_input(execution, step_id, &mut self.accepted_input)
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
        _cancel: &CancellationToken,
    ) -> Result<(), LoopEngineError> {
        CompactionCoordinator::from_context(self.runtime_context)
            .compact(execution, &mut self.observer)
            .await
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
    pub tool_context: tools::ToolExecutionContext,
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
    pub tool_context: tools::ToolExecutionContext,
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
            Some(execution.turn_count()),
            &format!("Interaction: id={}", request.id),
        );
        Ok(())
    }
}

pub(crate) struct RuntimeModelInvocation<O> {
    observer: O,
    advance_turn: bool,
}

impl<O> RuntimeModelInvocation<O>
where
    O: crate::application::model::invocation::ModelInvocationObserver,
{
    pub(crate) fn new(observer: O, advance_turn: bool) -> Self {
        Self {
            observer,
            advance_turn,
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
        cancel: &CancellationToken,
    ) -> Result<
        (
            crate::application::loop_engine::ModelStep,
            crate::application::loop_engine::StepTokenUsage,
        ),
        LoopEngineError,
    > {
        if self.advance_turn {
            execution.advance_turn();
        }
        let turn = execution.turn_count();
        logging::within(
            logging::LogContextPatch {
                turn: logging::FieldPatch::Set(turn),
                request_id: logging::FieldPatch::Clear,
                ..logging::LogContextPatch::default()
            },
            crate::application::model::invocation::orchestrate_model_invocation(
                &mut self.observer,
                execution,
                cancel,
            ),
        )
        .await
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

    async fn begin_stop_hook_status(&mut self) -> Result<(), LoopEngineError> {
        self.observer.begin_stop_hook_status().await
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

use tokio_util::sync::CancellationToken;

use crate::application::hook::stop_coordination::{StopHookObserver, StopHookOutcome};
use crate::application::loop_engine::{
    CompactionPort, EventSinkPort, InputPort, InteractionMailboxPort, InternalContinuationKind,
    LoopEngineError, ModelInvocationPort, PendingInteractionWork, PlanApprovalPort, RunControlPort,
    RunLifecyclePort, StepPersistencePort, StuckDecision, StuckHandlingPort, ToolOrchestrationPort,
};
use crate::application::run::execution_state::RunExecutionState;
use crate::domain::agent_run::RunDomainEvent;

/// 单次 Run 的 Loop Engine 调用上下文。
///
/// 该类型只负责调用顺序，不实现任何 Port，也不隐藏来源差异。每项依赖均以
/// 独立窄端口借用传入，避免任何单个来源对象重新聚合完整 Loop 能力。
pub struct RunLoop<'a> {
    input: &'a mut dyn InputPort,
    events: &'a mut dyn EventSinkPort,
    control: &'a dyn RunControlPort,
    lifecycle: &'a dyn RunLifecyclePort,
    interaction: &'a mut dyn InteractionMailboxPort,
    persistence: &'a mut dyn StepPersistencePort,
    compaction: &'a mut dyn CompactionPort,
    model: &'a mut dyn ModelInvocationPort,
    stop_hook: &'a mut dyn StopHookObserver,
    tools: &'a mut dyn ToolOrchestrationPort,
    stuck: &'a mut dyn StuckHandlingPort,
    plan_approval: &'a dyn PlanApprovalPort,
}

impl<'a> RunLoop<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        input: &'a mut dyn InputPort,
        events: &'a mut dyn EventSinkPort,
        control: &'a dyn RunControlPort,
        lifecycle: &'a dyn RunLifecyclePort,
        interaction: &'a mut dyn InteractionMailboxPort,
        persistence: &'a mut dyn StepPersistencePort,
        compaction: &'a mut dyn CompactionPort,
        model: &'a mut dyn ModelInvocationPort,
        stop_hook: &'a mut dyn StopHookObserver,
        tools: &'a mut dyn ToolOrchestrationPort,
        stuck: &'a mut dyn StuckHandlingPort,
        plan_approval: &'a dyn PlanApprovalPort,
    ) -> Self {
        Self {
            input,
            events,
            control,
            lifecycle,
            interaction,
            persistence,
            compaction,
            model,
            stop_hook,
            tools,
            stuck,
            plan_approval,
        }
    }

    pub(super) fn input_mut(&mut self) -> &mut dyn InputPort {
        self.input
    }

    pub(super) fn persistence_mut(&mut self) -> &mut dyn StepPersistencePort {
        self.persistence
    }

    pub(super) fn compaction_mut(&mut self) -> &mut dyn CompactionPort {
        self.compaction
    }

    pub(super) fn model_mut(&mut self) -> &mut dyn ModelInvocationPort {
        self.model
    }

    pub(super) fn tools_mut(&mut self) -> &mut dyn ToolOrchestrationPort {
        self.tools
    }

    pub(super) fn schedule_internal_continuation(&mut self, kind: InternalContinuationKind) {
        self.input.schedule_internal_continuation(kind);
    }

    pub(super) async fn emit(
        &mut self,
        execution: &mut RunExecutionState,
        events: Vec<RunDomainEvent>,
    ) -> Result<(), LoopEngineError> {
        self.events.emit(execution, events).await
    }

    pub(super) fn take_control(
        &self,
        run_id: &sdk::RunId,
    ) -> Option<crate::domain::agent_run::RunControl> {
        self.control.take_control(run_id)
    }

    pub(super) fn claim_terminal(&self, run_id: &sdk::RunId) -> bool {
        self.lifecycle.claim_terminal(run_id)
    }

    pub(super) fn claim_cancellation(&self, run_id: &sdk::RunId) -> bool {
        self.lifecycle.claim_cancellation(run_id)
    }

    pub(super) fn register_step_scope(
        &self,
        run_id: &sdk::RunId,
        step_id: sdk::RunStepId,
        cancel: CancellationToken,
    ) {
        self.lifecycle.register_step_scope(run_id, step_id, cancel);
    }

    pub(super) fn interaction_port(
        &self,
    ) -> &dyn crate::application::interaction::port::InteractionPort {
        self.interaction.interaction_port()
    }

    pub(super) async fn publish_interaction(
        &mut self,
        execution: &RunExecutionState,
        request: &sdk::InteractionRequest,
    ) -> Result<(), LoopEngineError> {
        self.interaction
            .publish_interaction(execution, request)
            .await
    }

    pub(super) fn set_pending_interaction_work(
        &mut self,
        execution: &mut RunExecutionState,
        work: PendingInteractionWork,
    ) {
        self.interaction
            .set_pending_interaction_work(execution, work);
    }

    pub(super) fn interaction_completion_context(
        &self,
        step_cancel: CancellationToken,
    ) -> crate::application::interaction::coordinator::InteractionCompletionContext<'_> {
        self.interaction.interaction_completion_context(step_cancel)
    }

    pub(super) async fn needs_compaction(
        &mut self,
        execution: &mut RunExecutionState,
    ) -> Result<bool, LoopEngineError> {
        self.compaction.needs_compaction(execution).await
    }

    pub(super) async fn coordinate_stop_hook(
        &mut self,
        execution: &mut RunExecutionState,
        turns: usize,
        cancel: &CancellationToken,
    ) -> Result<StopHookOutcome, LoopEngineError> {
        crate::application::hook::stop_coordination::coordinate_stop_hook(
            self.stop_hook,
            execution,
            turns,
            cancel,
        )
        .await
    }

    pub(super) async fn on_stuck(
        &mut self,
        execution: &RunExecutionState,
        decision: &StuckDecision,
    ) -> Result<(), LoopEngineError> {
        self.stuck.on_stuck(execution, decision).await
    }

    pub(super) fn needs_plan_approval(&self) -> bool {
        self.plan_approval.needs_plan_approval()
    }
}

use std::future::Future;
use std::time::Instant;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::application::hook::stop_coordination::StopHookDecision;
use crate::application::loop_engine::RunLoop;
use crate::application::run::context::RuntimeContext;
use crate::application::run::execution_state::RunExecutionState;
use crate::application::tool::agent::ToolCall;
use crate::domain::agent_run::{
    DrainDecision, InteractionContinuation, ModelInvocation, Run, RunCancellationRequest,
    RunDomainEvent, RunStatus, RunTransition, RunTransitionError, StopHookBlockResult,
    ToolCallStatus,
};

use super::{StuckDecision, StuckGuard};

/// Monotonic per-Run drain epoch. Each successful drain call increments
/// the epoch. Callers pass their expected epoch for mismatch detection
/// (#1272 per-turn drain-or-seal linearization).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DrainEpoch(pub u64);

impl DrainEpoch {
    /// Advance to the next epoch.
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopInput {
    pub text: String,
    /// Per-turn user message InputId (from `ChatInputEvent::UserMessage::id`).
    /// `None` for engine-driven continuations (StopHookFeedback, ToolResults)
    /// and fixed-sub-agent prompts (#1272 per-turn drain identity).
    pub input_id: Option<sdk::InputId>,
    /// Per-turn user message images (from `ChatInputEvent::UserMessage::images`).
    /// Empty for engine-driven continuations.
    pub images: Vec<sdk::ChatInputImage>,
}

#[derive(Debug, Clone, Default)]
pub struct StepTokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cache_creation_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
    pub context_window: u64,
    /// 估算：system prompt tokens
    pub est_system_tokens: usize,
    /// 估算：tool schemas tokens
    pub est_tool_tokens: usize,
    /// 估算：messages tokens
    pub est_message_tokens: usize,
    /// API 返回的 stop_reason（如 "end_turn" / "max_tokens" / "tool_use"）
    pub stop_reason: String,
}

impl StepTokenUsage {
    /// 估算总量（system + tools + messages）
    pub fn est_total(&self) -> usize {
        self.est_system_tokens + self.est_tool_tokens + self.est_message_tokens
    }
}

#[derive(Clone)]
pub enum ModelStep {
    Complete {
        text: String,
    },
    #[cfg(test)]
    Continue {
        text: String,
    },
    Tools {
        text: String,
        calls: Vec<ToolCall>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolGuardDecision {
    Allow,
    SoftBlock { reason: String },
}

/// #1248 Task 5: A tool call that was suspended for user interaction.
/// Carries the suspension details needed to form a `UserQuestions` intent.
#[derive(Debug, Clone, PartialEq)]
pub struct SuspendedToolCall {
    pub call: ToolCall,
    pub questions: Vec<SuspendedQuestion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuspendedQuestion {
    pub prompt: String,
    pub options: Vec<String>,
    pub allow_multi: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolStep {
    Continue,
    ContinueWithFuseBypass(Vec<sdk::ToolCallId>),
    #[cfg(test)]
    AwaitUser,
    /// #1248 Task 5: Tool execution produced one or more suspensions
    /// that should be resolved through the interaction coordinator.
    InteractionSuspended {
        suspended: Vec<SuspendedToolCall>,
        /// #1248: Completed (non-interaction) results from this round.
        /// The port already recorded them as messages/events; the engine
        /// must advance these tool calls to their final status.
        completed_results: Vec<(sdk::ToolCallId, ToolCallStatus)>,
        fuse_bypassed: Vec<sdk::ToolCallId>,
    },
    /// #1248 Task 5: Some tool calls require approval before execution.
    /// The engine creates ToolApproval intents for each.
    AwaitingToolApproval {
        calls_needing_approval: Vec<ApprovalRequiredCall>,
        /// #1248: Completed (non-interaction) results from this round.
        completed_results: Vec<(sdk::ToolCallId, ToolCallStatus)>,
        fuse_bypassed: Vec<sdk::ToolCallId>,
    },
}

/// #1248 Task 5: A tool call that needs approval before execution.
/// Carries the full ToolCall and AuthorizationContext so the approval
/// flow can execute directly without re-evaluating policy.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalRequiredCall {
    pub call: ToolCall,
    pub authorization: tools::AuthorizationContext,
    pub reason: String,
    pub subject: String,
}

/// #1248: Outcome of `finish_interaction_work` on the port.
/// Tells the engine what happened so it can advance tool calls
/// and decide whether to start the next interaction or complete the step.
#[derive(Debug)]
pub enum InteractionWorkOutcome {
    /// Interaction resolved normally — the port wrote tool result messages,
    /// sent ToolResult events, and marked the result.
    Completed {
        /// The tool call id that was resolved.
        call_id: sdk::ToolCallId,
        /// Final tool call status (Success or Error).
        status: ToolCallStatus,
        /// Remaining interaction items still in the queue (excluding the resolved one).
        remaining_queue: Vec<PendingInteractionItem>,
        /// Whether the materialized result should trigger the next model invocation.
        schedule_tool_results: bool,
    },
}

/// #1248: A single pending interaction item in the port's queue.
/// The engine writes the queue when more than one interaction is needed
/// in a single round, and the port reads it back on each `finish_interaction_work`
/// call so the outcome carries the updated queue.
#[derive(Debug, Clone)]
pub struct PendingInteractionItem {
    /// The request_id for this interaction (matches metadata.request_id).
    pub request_id: sdk::InteractionRequestId,
    /// Continuation type from when the interaction was created.
    pub continuation: InteractionContinuation,
    /// For suspensions: the suspended tool call (present if UserQuestions).
    pub suspended_call: Option<SuspendedToolCall>,
    /// For approvals: the approval call (present if ToolApproval).
    pub approval_call: Option<ApprovalRequiredCall>,
}

/// #1248: Pending interaction work stored on the port when a round produces
/// more than one interaction-worthy call.  The engine writes this once per
/// round; the port drains items as each resolves.
#[derive(Debug, Clone, Default)]
pub struct PendingInteractionWork {
    /// The interaction currently being resolved (the one that was started via
    /// coordinator and is now being finished).  Holds the full SuspendedToolCall
    /// or ApprovalRequiredCall so `finish_interaction_work` can use the original
    /// call's provider_id/name and, for approvals, execute the tool directly.
    pub current: Option<PendingInteractionItem>,
    /// Remaining interaction queue (items not yet started).
    /// Front = next to start after `current` resolves.
    pub queue: Vec<PendingInteractionItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopDirective {
    Terminal,
    AwaitUser,
}

/// Returned by `drain_input` to tell the engine what to do next.
///
/// #1272 Per-turn drain-or-seal contract:
/// - `Ready` carries a **non-empty** batch of user input.
/// - `InternalContinuation` is for engine-driven continuations: stop hook
///   feedback or recorded tool results. The batch can be empty (pure
///   continuation) or carry any user input that arrived alongside it.
/// - `EmptyAndSealed` is the unique terminal gate.
///
/// Each variant carries a [`DrainEpoch`] for per-turn linearization.
#[derive(Debug, Clone)]
pub enum DrainOutcome {
    /// User input is ready for the next step. The batch SHOULD be non-empty;
    /// use [`DrainOutcome::ready`] to construct. An empty batch is detected
    /// by `run_loop` and reported as `LoopEngineError::Adapter` (#1272).
    Ready {
        batch: Vec<LoopInput>,
        epoch: DrainEpoch,
    },
    /// Engine-driven continuation (stop-hook feedback or tool results).
    InternalContinuation {
        kind: InternalContinuationKind,
        batch: Vec<LoopInput>,
        epoch: DrainEpoch,
    },
    /// No more work: seal the Run and transition to Completed.
    EmptyAndSealed { epoch: DrainEpoch },
    /// No user input available while awaiting user. Buffer is not sealed;
    /// epoch is not advanced. Caller should return AwaitUser and retry
    /// with the same expected epoch (#1272).
    NoInput { epoch: DrainEpoch },
}

impl DrainOutcome {
    /// Construct a `Ready` outcome. Does not panic on empty batch — an
    /// empty `Ready` is detected by `run_loop` at the shared consumption
    /// point and reported as `LoopEngineError::Adapter` (#1272 close-out).
    /// Adapters should still avoid producing empty Ready; for no-work seal
    /// use `DrainOutcome::EmptyAndSealed` directly.
    #[cfg(test)]
    pub fn ready(batch: Vec<LoopInput>, epoch: DrainEpoch) -> Self {
        Self::Ready { batch, epoch }
    }

    /// The epoch carried by this outcome — used by the engine to validate
    /// per-turn linearization (#1272).
    pub fn epoch(&self) -> DrainEpoch {
        match self {
            Self::Ready { epoch, .. }
            | Self::InternalContinuation { epoch, .. }
            | Self::EmptyAndSealed { epoch }
            | Self::NoInput { epoch } => *epoch,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalContinuationKind {
    /// Stop hook produced feedback; the model should see it as a system prefix.
    StopHookFeedback { feedback: String },
    /// Tool results have been recorded; the model should read them.
    ToolResults,
}

#[derive(Debug, thiserror::Error)]
pub enum LoopEngineError {
    #[error("run state error: {0}")]
    Domain(#[from] RunTransitionError),
    #[error("loop adapter requested context compaction: {0}")]
    NeedsCompaction(String),
    #[error("loop adapter error: {0}")]
    Adapter(String),
    #[error("loop operation cancelled")]
    Cancelled,
}

#[async_trait]
pub trait InputPort: Send {
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError>;

    fn schedule_internal_continuation(&mut self, _kind: InternalContinuationKind) {}

    /// Drain input while the Run is AwaitingUser without sealing the input source.
    async fn await_user_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        log::debug!(
            target: crate::LOG_TARGET,
            "InputPort::await_user_input 使用默认实现（epoch {:?}）：该 adapter 未覆写等待输入能力",
            expected_epoch,
        );
        Err(LoopEngineError::Adapter(format!(
            "该 adapter 未覆写 await_user_input（epoch {:?}）：可进入 AwaitingUser 的 adapter 必须实现该方法，保证空输入时返回 NoInput 而非 seal buffer",
            expected_epoch,
        )))
    }
}

#[async_trait]
pub trait EventSinkPort: Send {
    async fn emit(
        &mut self,
        execution: &mut RunExecutionState,
        events: Vec<RunDomainEvent>,
    ) -> Result<(), LoopEngineError>;
}

#[async_trait]
pub trait RunControlPort: Send + Sync {
    fn take_control(&self, run_id: &sdk::RunId) -> Option<crate::domain::agent_run::RunControl>;
}

#[async_trait]
pub trait RunLifecyclePort: Send + Sync {
    fn claim_terminal(&self, run_id: &sdk::RunId) -> bool;
    fn claim_cancellation(&self, run_id: &sdk::RunId) -> bool;
    fn register_step_scope(
        &self,
        run_id: &sdk::RunId,
        step_id: sdk::RunStepId,
        cancel: CancellationToken,
    );
    fn clear_step_scope(&self, run_id: &sdk::RunId, step_id: &sdk::RunStepId);
    fn clear_cancelled_step_scope(&self, run_id: &sdk::RunId, step_id: &sdk::RunStepId);
}

#[async_trait]
pub trait InteractionMailboxPort:
    crate::application::interaction::coordinator::InteractionCompletionContextProvider + Send
{
    fn interaction_port(&self) -> &dyn crate::application::interaction::port::InteractionPort {
        static UNAVAILABLE: std::sync::LazyLock<
            crate::application::interaction::port::UnavailableInteractionPort,
        > = std::sync::LazyLock::new(
            crate::application::interaction::port::UnavailableInteractionPort::default,
        );
        &*UNAVAILABLE
    }
    async fn publish_interaction(
        &mut self,
        _execution: &RunExecutionState,
        _request: &sdk::InteractionRequest,
    ) -> Result<(), LoopEngineError> {
        Ok(())
    }
    fn set_pending_interaction_work(
        &mut self,
        _execution: &mut RunExecutionState,
        _work: PendingInteractionWork,
    ) {
    }
}

fn loop_input_message(input: &LoopInput) -> share::message::Message {
    if input.images.is_empty() {
        return share::message::Message::user(input.text.clone());
    }
    share::message::Message::user_with_images(
        input.text.clone(),
        input
            .images
            .iter()
            .cloned()
            .map(|image| (image.id, image.base64, image.media_type))
            .collect(),
    )
}

fn freeze_step<P>(
    execution: &mut RunExecutionState,
    persistence: &mut P,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    inputs: &[LoopInput],
) where
    P: StepPersistencePort + ?Sized,
{
    persistence.observe_step_frozen(step_id);
    let prefix = persistence.take_step_input_prefix();
    let has_prefix = prefix.is_some();
    execution.freeze_step_input_messages(prefix, inputs.iter().map(loop_input_message).collect());
    if has_prefix {
        execution.extend_messages(inputs.iter().map(loop_input_message));
    }
    execution.replace_adopted_input(
        inputs
            .iter()
            .filter_map(|input| {
                input
                    .input_id
                    .as_ref()
                    .map(|id| (id.clone(), loop_input_message(input)))
            })
            .collect(),
    );
    if let Some(request) = persistence.build_context_request(execution, run_id, step_id) {
        execution.replace_context_state(request, None);
    }
}

#[derive(Debug, Clone)]
pub struct StepCommit {
    pub request: Option<crate::ports::ContextRequest>,
    pub step_id: sdk::RunStepId,
    pub expected_revision: Option<crate::ports::SessionRevision>,
    pub cause: crate::ports::FinalizeCause,
    pub messages: Vec<share::message::Message>,
}

fn prepare_step_commit(
    execution: &RunExecutionState,
    step_id: &sdk::RunStepId,
    cause: crate::ports::FinalizeCause,
) -> StepCommit {
    let request = execution.context_request().cloned();
    if let Some(request) = request.as_ref() {
        debug_assert_eq!(&request.step_id, step_id);
    }
    StepCommit {
        request,
        step_id: step_id.clone(),
        expected_revision: execution
            .context_window()
            .map(|window| window.backing_revision),
        cause,
        messages: execution.step_outcome(),
    }
}

async fn finalize_step<P>(
    execution: &mut RunExecutionState,
    persistence: &mut P,
    step_id: &sdk::RunStepId,
    cause: crate::ports::FinalizeCause,
) -> Result<(), LoopEngineError>
where
    P: StepPersistencePort + ?Sized,
{
    let commit = prepare_step_commit(execution, step_id, cause);
    persistence.persist_step_commit(&commit).await?;
    execution.commit_step_messages();
    Ok(())
}

#[async_trait]
pub trait StepPersistencePort: Send {
    fn observe_step_frozen(&mut self, _step_id: &sdk::RunStepId) {}
    fn take_step_input_prefix(&mut self) -> Option<share::message::Message> {
        None
    }
    fn build_context_request(
        &self,
        _execution: &RunExecutionState,
        _run_id: &sdk::RunId,
        _step_id: &sdk::RunStepId,
    ) -> Option<crate::ports::ContextRequest> {
        None
    }
    async fn accept_step_input(
        &mut self,
        _execution: &mut RunExecutionState,
        _step_id: &sdk::RunStepId,
    ) -> Result<(), LoopEngineError> {
        Ok(())
    }
    async fn persist_step_commit(&mut self, _commit: &StepCommit) -> Result<(), LoopEngineError> {
        Ok(())
    }
}

#[async_trait]
pub trait CompactionPort: Send {
    async fn needs_compaction(
        &mut self,
        execution: &mut RunExecutionState,
    ) -> Result<bool, LoopEngineError>;
    async fn compact(
        &mut self,
        execution: &mut RunExecutionState,
        cancel: &CancellationToken,
    ) -> Result<(), LoopEngineError>;
}

#[async_trait]
pub trait ModelInvocationPort: Send {
    async fn invoke_model(
        &mut self,
        execution: &mut RunExecutionState,
        cancel: &CancellationToken,
    ) -> Result<(ModelStep, StepTokenUsage), LoopEngineError>;
}

#[async_trait]
pub trait ToolOrchestrationPort: Send {
    async fn execute_tools(
        &mut self,
        execution: &mut RunExecutionState,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        calls: &[(ToolCall, ToolGuardDecision)],
        cancel: &CancellationToken,
    ) -> Result<crate::application::tool::coordination::ToolRoundOutcome, LoopEngineError>;
}

#[async_trait]
pub trait StuckHandlingPort: Send {
    async fn on_stuck(
        &mut self,
        execution: &RunExecutionState,
        decision: &StuckDecision,
    ) -> Result<(), LoopEngineError>;
}

pub trait PlanApprovalPort: Send + Sync {
    fn needs_plan_approval(&self) -> bool {
        false
    }
}

enum InputDrainOutcome {
    Drained(DrainOutcome),
    Cancelled,
    TimedOut,
}

enum AwaitingRunIngress {
    Input(InputDrainOutcome),
    Interaction(crate::application::interaction::port::InteractionResolution),
}

fn drain_outcome_kind(outcome: &DrainOutcome) -> &'static str {
    match outcome {
        DrainOutcome::Ready { .. } => "ready",
        DrainOutcome::InternalContinuation { .. } => "internal_continuation",
        DrainOutcome::EmptyAndSealed { .. } => "empty_and_sealed",
        DrainOutcome::NoInput { .. } => "no_input",
    }
}

fn interaction_resolution_kind(
    resolution: &crate::application::interaction::port::InteractionResolution,
) -> &'static str {
    use crate::application::interaction::port::{InteractionCompletion, InteractionResolution};

    match resolution {
        InteractionResolution::Resolved {
            completion: InteractionCompletion::Replied(_),
            ..
        } => "replied",
        InteractionResolution::Resolved {
            completion: InteractionCompletion::Cancelled(_),
            ..
        } => "cancelled",
        InteractionResolution::Closed { .. } => "closed",
    }
}

async fn run_input_drain_phase<P>(
    run: &Run,
    awaiting_user: bool,
    expected_epoch: DrainEpoch,
    cancel: &CancellationToken,
    input: &mut P,
) -> Result<InputDrainOutcome, LoopEngineError>
where
    P: InputPort + ?Sized,
{
    let drain_future = if awaiting_user {
        input.await_user_input(expected_epoch)
    } else {
        input.drain_input(expected_epoch)
    };
    match await_interruptible(run, cancel, drain_future).await {
        Interrupt::Completed(result) => result.map(InputDrainOutcome::Drained),
        Interrupt::Cancelled => Ok(InputDrainOutcome::Cancelled),
        Interrupt::TimedOut => Ok(InputDrainOutcome::TimedOut),
    }
}

async fn await_run_ingress<P>(
    run: &Run,
    execution: &mut RunExecutionState,
    expected_epoch: DrainEpoch,
    cancel: &CancellationToken,
    input: &mut P,
) -> Result<AwaitingRunIngress, LoopEngineError>
where
    P: InputPort + ?Sized,
{
    let Some(mut active_interaction) = execution.take_active_interaction() else {
        return Err(LoopEngineError::Adapter(
            "Run 处于 AwaitingUser 且声明 pending interaction，但 interaction mailbox 为空"
                .to_string(),
        ));
    };
    let run_id = active_interaction.metadata.run_id.clone();
    let request_id = active_interaction.metadata.request_id.clone();
    let input_future = run_input_drain_phase(run, true, expected_epoch, cancel, input);
    tokio::pin!(input_future);

    tokio::select! {
        biased;
        completion = &mut active_interaction.receiver => {
            let resolution = match completion {
                Ok(completion) => {
                    crate::application::interaction::port::InteractionResolution::Resolved {
                        metadata: active_interaction.metadata,
                        completion,
                    }
                }
                Err(_) => crate::application::interaction::port::InteractionResolution::Closed {
                    metadata: active_interaction.metadata,
                },
            };
            log::debug!(
                target: crate::LOG_TARGET,
                "[run_loop] unified ingress woke by interaction run_id={} request_id={} resolution={}",
                run_id,
                request_id,
                interaction_resolution_kind(&resolution),
            );
            Ok(AwaitingRunIngress::Interaction(resolution))
        }
        input_outcome = &mut input_future => {
            let input_outcome = input_outcome?;
            log::debug!(
                target: crate::LOG_TARGET,
                "[run_loop] unified ingress woke by session input run_id={} request_id={}",
                run_id,
                request_id,
            );
            crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
                execution,
                active_interaction.metadata,
                active_interaction.receiver,
            )
            .map_err(|error| {
                LoopEngineError::Adapter(format!(
                    "interaction mailbox restore failed: {error:?}"
                ))
            })?;
            Ok(AwaitingRunIngress::Input(input_outcome))
        }
    }
}

enum StepInputOutcome {
    Accepted(sdk::RunStepId),
    Rejected(LoopEngineError),
}

async fn run_step_input_phase<P>(
    run: &mut Run,
    execution: &mut RunExecutionState,
    step_id: sdk::RunStepId,
    inputs: &[LoopInput],
    persistence: &mut P,
) -> Result<StepInputOutcome, LoopEngineError>
where
    P: StepPersistencePort + ?Sized,
{
    execution.begin_step();
    freeze_step(execution, persistence, run.id(), &step_id, inputs);
    match persistence.accept_step_input(execution, &step_id).await {
        Ok(()) => Ok(StepInputOutcome::Accepted(run.begin_step_with_id(step_id)?)),
        Err(error) => Ok(StepInputOutcome::Rejected(error)),
    }
}

enum StepFinalizationOutcome {
    Committed,
}

async fn run_step_finalization_phase<P>(
    execution: &mut RunExecutionState,
    persistence: &mut P,
    step_id: &sdk::RunStepId,
    cause: crate::ports::FinalizeCause,
) -> Result<StepFinalizationOutcome, LoopEngineError>
where
    P: StepPersistencePort + ?Sized,
{
    finalize_step(execution, persistence, step_id, cause).await?;
    Ok(StepFinalizationOutcome::Committed)
}

#[derive(Debug)]
enum ContextCompactionOutcome {
    Ready,
    Cancelled,
    TimedOut,
}

async fn run_context_compaction_phase<P>(
    run: &Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    compaction: &mut P,
) -> Result<ContextCompactionOutcome, LoopEngineError>
where
    P: CompactionPort + ?Sized,
{
    match await_interruptible(run, cancel, compaction.compact(execution, cancel)).await {
        Interrupt::Completed(Ok(())) => Ok(ContextCompactionOutcome::Ready),
        Interrupt::Completed(Err(LoopEngineError::Cancelled)) | Interrupt::Cancelled => {
            Ok(ContextCompactionOutcome::Cancelled)
        }
        Interrupt::Completed(Err(error)) => Err(error),
        Interrupt::TimedOut => Ok(ContextCompactionOutcome::TimedOut),
    }
}

enum ModelInvocationOutcome {
    Invoked(ModelStep, StepTokenUsage),
    NeedsCompaction(String),
    Failed(LoopEngineError),
    Cancelled,
    TimedOut,
}

async fn run_model_invocation_phase<P>(
    run: &Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    model: &mut P,
) -> ModelInvocationOutcome
where
    P: ModelInvocationPort + ?Sized,
{
    match await_interruptible(run, cancel, model.invoke_model(execution, cancel)).await {
        Interrupt::Completed(Ok((step, usage))) => ModelInvocationOutcome::Invoked(step, usage),
        Interrupt::Completed(Err(LoopEngineError::NeedsCompaction(error))) => {
            ModelInvocationOutcome::NeedsCompaction(error)
        }
        Interrupt::Completed(Err(LoopEngineError::Cancelled)) | Interrupt::Cancelled => {
            ModelInvocationOutcome::Cancelled
        }
        Interrupt::Completed(Err(error)) => ModelInvocationOutcome::Failed(error),
        Interrupt::TimedOut => ModelInvocationOutcome::TimedOut,
    }
}

#[derive(Debug)]
enum ToolRoundPhaseOutcome {
    Completed(crate::application::tool::coordination::ToolRoundOutcome),
    Failed(LoopEngineError),
    Cancelled,
    TimedOut,
}

async fn run_tool_round_phase<P>(
    run: &Run,
    execution: &mut RunExecutionState,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    calls: &[(ToolCall, ToolGuardDecision)],
    cancel: &CancellationToken,
    tools: &mut P,
) -> ToolRoundPhaseOutcome
where
    P: ToolOrchestrationPort + ?Sized,
{
    match await_interruptible(
        run,
        cancel,
        tools.execute_tools(execution, run_id, step_id, calls, cancel),
    )
    .await
    {
        Interrupt::Completed(Ok(outcome)) => ToolRoundPhaseOutcome::Completed(outcome),
        Interrupt::Completed(Err(LoopEngineError::Cancelled)) | Interrupt::Cancelled => {
            ToolRoundPhaseOutcome::Cancelled
        }
        Interrupt::Completed(Err(error)) => ToolRoundPhaseOutcome::Failed(error),
        Interrupt::TimedOut => ToolRoundPhaseOutcome::TimedOut,
    }
}

enum Interrupt<T> {
    Completed(T),
    Cancelled,
    TimedOut,
}

async fn await_interruptible<F, T>(run: &Run, cancel: &CancellationToken, future: F) -> Interrupt<T>
where
    F: Future<Output = T>,
{
    if let Some(remaining) = run.remaining_time(Instant::now()) {
        if remaining.is_zero() {
            return Interrupt::TimedOut;
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Interrupt::Cancelled,
            _ = tokio::time::sleep(remaining) => Interrupt::TimedOut,
            value = future => Interrupt::Completed(value),
        }
    } else {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Interrupt::Cancelled,
            value = future => Interrupt::Completed(value),
        }
    }
}

/// Unified production-shaped engine entry.
pub async fn execute_prepared_loop(
    run: &mut Run,
    execution: &mut RunExecutionState,
    context: &RuntimeContext,
    cancel: &CancellationToken,
    loop_context: &mut RunLoop<'_>,
) -> Result<LoopDirective, LoopEngineError> {
    let result = run_loop(run, execution, cancel, loop_context).await;

    let _event_sink = context.event_sink();
    result
}

pub async fn run_loop(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
) -> Result<LoopDirective, LoopEngineError> {
    if run.status() == RunStatus::Created {
        run.start_draining()?;
        emit_events(run, execution, port).await?;
    }

    log::debug!(
        target: crate::LOG_TARGET,
        "[run_loop] entered run_id={} parent={} spec={:?}",
        run.id(),
        run.parent_id().map(|id| id.to_string()).unwrap_or_else(|| "none".into()),
        run.spec(),
    );

    let mut guard = StuckGuard::new();
    // #1272: engine-owned epoch for per-turn drain linearization.
    // Initialized from the Run's persisted epoch so that re-entering
    // run_loop (e.g. after AwaitUser) recovers the correct epoch
    // instead of resetting to 0.  Each successful drain increments
    // both the engine-local counter and the Run's persisted epoch.
    let mut expected_epoch = DrainEpoch(run.next_drain_epoch());
    // #1272: collect the last assistant text for terminal claim across
    // loop iterations. Every model step response is tracked; the last
    // one before EmptyAndSealed becomes the terminal text carried in the
    // Completed event. Must live outside the loop block — otherwise
    // Complete→drain→EmptyAndSealed loses the result.
    let mut terminal_text: Option<String> = None;
    loop {
        if let Some(control) = handle_pending_control(run, execution, port).await? {
            if matches!(control, ControlDirective::Terminal) {
                return Ok(LoopDirective::Terminal);
            }
            continue;
        }
        if handle_interrupt(run, execution, cancel, port).await? {
            return Ok(LoopDirective::Terminal);
        }
        // #1272: failed/cancelled runs are terminal; do not drain again.
        if run.status().is_terminal() {
            return Ok(LoopDirective::Terminal);
        }

        // AwaitingUser with an active Interaction has two legitimate ingress
        // sources. Wait for both in one select so either source can wake the
        // Run; never park exclusively on the Session input mailbox.
        let awaiting_user = run.status() == RunStatus::AwaitingUser;
        let outcome = if awaiting_user && run.pending_interaction().is_some() {
            let pending = run
                .pending_interaction()
                .expect("checked pending interaction above");
            log::debug!(
                target: crate::LOG_TARGET,
                "[run_loop] awaiting unified ingress run_id={} request_id={} continuation={:?}",
                run.id(),
                pending.request_id,
                pending.continuation,
            );
            match await_run_ingress(run, execution, expected_epoch, cancel, port.input_mut())
                .await?
            {
                AwaitingRunIngress::Interaction(completion) => {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[run_loop] interaction completion observed run_id={} request_id={} resolution={}",
                        run.id(),
                        completion.metadata().request_id,
                        interaction_resolution_kind(&completion),
                    );
                    handle_interaction_completion(run, execution, port, cancel, completion).await?;
                    continue;
                }
                AwaitingRunIngress::Input(input_outcome) => match input_outcome {
                    InputDrainOutcome::Drained(outcome) => {
                        log::debug!(
                            target: crate::LOG_TARGET,
                            "[run_loop] input drain completed run_id={} status={:?} pending_interaction={} outcome={}",
                            run.id(),
                            run.status(),
                            run.pending_interaction().is_some(),
                            drain_outcome_kind(&outcome),
                        );
                        outcome
                    }
                    InputDrainOutcome::Cancelled => {
                        if let Some(control) = handle_pending_control(run, execution, port).await? {
                            return Ok(match control {
                                ControlDirective::Continue => LoopDirective::AwaitUser,
                                ControlDirective::Terminal => LoopDirective::Terminal,
                            });
                        }
                        cancel_run(run, execution, port).await?;
                        return Ok(LoopDirective::Terminal);
                    }
                    InputDrainOutcome::TimedOut => {
                        timeout_run(run, execution, port).await?;
                        return Ok(LoopDirective::Terminal);
                    }
                },
            }
        } else {
            log::debug!(
                target: crate::LOG_TARGET,
                "[run_loop] input drain starting run_id={} status={:?} awaiting_user={} pending_interaction={} epoch={:?}",
                run.id(),
                run.status(),
                awaiting_user,
                run.pending_interaction().is_some(),
                expected_epoch,
            );
            match run_input_drain_phase(
                run,
                awaiting_user,
                expected_epoch,
                cancel,
                port.input_mut(),
            )
            .await?
            {
                InputDrainOutcome::Drained(outcome) => {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[run_loop] input drain completed run_id={} status={:?} pending_interaction={} outcome={}",
                        run.id(),
                        run.status(),
                        run.pending_interaction().is_some(),
                        drain_outcome_kind(&outcome),
                    );
                    outcome
                }
                InputDrainOutcome::Cancelled => {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[run_loop] input drain interrupted run_id={} result=cancelled status={:?} pending_interaction={}",
                        run.id(),
                        run.status(),
                        run.pending_interaction().is_some(),
                    );
                    if let Some(control) = handle_pending_control(run, execution, port).await? {
                        return Ok(match control {
                            ControlDirective::Continue => LoopDirective::AwaitUser,
                            ControlDirective::Terminal => LoopDirective::Terminal,
                        });
                    }
                    cancel_run(run, execution, port).await?;
                    return Ok(LoopDirective::Terminal);
                }
                InputDrainOutcome::TimedOut => {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[run_loop] input drain interrupted run_id={} result=timed_out status={:?} pending_interaction={}",
                        run.id(),
                        run.status(),
                        run.pending_interaction().is_some(),
                    );
                    timeout_run(run, execution, port).await?;
                    return Ok(LoopDirective::Terminal);
                }
            }
        };

        // #1272: validate that the adapter returned the epoch the engine expects.
        if outcome.epoch() != expected_epoch {
            return Err(LoopEngineError::Adapter(format!(
                "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                expected_epoch,
                outcome.epoch(),
            )));
        }

        match outcome {
            DrainOutcome::Ready { batch, .. } => {
                // #1272 close-out: an empty Ready batch is a contract
                // violation (Ready must carry non-empty user input).
                // Detect it here — before any epoch advance or state
                // transition — and return a descriptive Adapter error
                // instead of panicking.
                if batch.is_empty() {
                    log::error!(
                        target: crate::LOG_TARGET,
                        "[run_loop] adapter 返回了空 Ready batch（epoch {:?}），\
                         这违反了 Ready 必须携带非空用户输入的契约",
                        expected_epoch,
                    );
                    return Err(LoopEngineError::Adapter(format!(
                        "drain_or_seal 在 epoch {:?} 返回了空的 Ready batch：\
                         Ready 必须携带非空用户输入，请改用 EmptyAndSealed 或 NoInput",
                        expected_epoch,
                    )));
                }
                // #1272: advance epoch BEFORE apply_drain_decision so that
                // epoch is incremented even if the decision fails (the
                // buffer already advanced its epoch; keeping them in sync
                // prevents a poisoned epoch on failure retry).
                run.advance_drain_epoch();
                expected_epoch = expected_epoch.next();

                // User input: resume if awaiting, then drain into work.
                if run.status() == RunStatus::AwaitingUser {
                    run.transition(RunTransition::UserResumed)?;
                }
                // batch is non-empty per DrainOutcome::Ready contract
                run.apply_drain_decision(DrainDecision::Inputs, None)?;
                execute_step(
                    run,
                    execution,
                    cancel,
                    port,
                    &mut guard,
                    &batch,
                    &mut terminal_text,
                )
                .await?;
            }
            DrainOutcome::InternalContinuation {
                kind: _kind, batch, ..
            } => {
                // #1272: InternalContinuation always advances epoch because
                // `take_internal_continuation` already advanced the buffer's
                // epoch — the continuation itself is a drain event, even
                // when the batch is empty.
                run.advance_drain_epoch();
                expected_epoch = expected_epoch.next();

                if run.status() == RunStatus::AwaitingUser {
                    // #1272: InternalContinuation without user input while
                    // awaiting user — do not auto-resume.  Only Ready
                    // (which guarantees a non-empty batch) resumes from
                    // AwaitingUser.  Return AwaitUser (epoch already
                    // advanced for the continuation).
                    if batch.is_empty() {
                        return Ok(LoopDirective::AwaitUser);
                    }
                    run.transition(RunTransition::UserResumed)?;
                }
                run.apply_drain_decision(DrainDecision::InternalContinuation, None)?;
                execute_step(
                    run,
                    execution,
                    cancel,
                    port,
                    &mut guard,
                    &batch,
                    &mut terminal_text,
                )
                .await?;
            }
            DrainOutcome::NoInput { .. } => {
                // #1272: NoInput from await_user_input — buffer is NOT
                // sealed, epoch is NOT advanced. Return AwaitUser so the
                // caller can wait for user input and re-enter with the
                // same expected epoch.
                debug_assert!(
                    awaiting_user,
                    "NoInput should only be produced by await_user_input (AwaitingUser state)"
                );
                return Ok(LoopDirective::AwaitUser);
            }
            DrainOutcome::EmptyAndSealed { .. } => {
                if run.status() == RunStatus::AwaitingUser {
                    // #1272: No user input pending; stay awaiting without
                    // advancing epoch — the buffer was sealed by the legacy
                    // path but this code path is still reachable from
                    // adapters whose await_user_input falls back to drain_input.
                    return Ok(LoopDirective::AwaitUser);
                }
                // #1272: advance epoch before apply_drain_decision.
                run.advance_drain_epoch();
                #[allow(unused_assignments)]
                {
                    expected_epoch = expected_epoch.next();
                }

                // #1272: terminal claim exactly once per run, at the seal point.
                if !port.claim_terminal(run.id()) {
                    cancel_run(run, execution, port).await?;
                    return Ok(LoopDirective::Terminal);
                }
                let text = terminal_text.as_deref();
                run.apply_drain_decision(DrainDecision::EmptyAndSealed, text)?;
                emit_events(run, execution, port).await?;
                return Ok(LoopDirective::Terminal);
            }
        }
    }
}

/// Execute one step: freeze input → build context → compact → invoke model →
/// handle response. Updates `terminal_text` with the last assistant text.
async fn execute_step(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
    guard: &mut StuckGuard,
    inputs: &[LoopInput],
    terminal_text: &mut Option<String>,
) -> Result<(), LoopEngineError> {
    let step_cancel = cancel.child_token();
    let step_id = sdk::RunStepId::new_v7();
    port.register_step_scope(run.id(), step_id.clone(), step_cancel.clone());
    let result = execute_step_with_scope(
        run,
        execution,
        cancel,
        port,
        guard,
        inputs,
        terminal_text,
        step_id.clone(),
        step_cancel,
    )
    .await;
    port.clear_step_scope(run.id(), &step_id);
    result
}

#[allow(clippy::too_many_arguments)]
async fn execute_step_with_scope(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
    guard: &mut StuckGuard,
    inputs: &[LoopInput],
    terminal_text: &mut Option<String>,
    step_id: sdk::RunStepId,
    step_cancel: CancellationToken,
) -> Result<(), LoopEngineError> {
    let step_id = match run_step_input_phase(
        run,
        execution,
        step_id,
        inputs,
        port.persistence_mut(),
    )
    .await?
    {
        StepInputOutcome::Accepted(step_id) => step_id,
        StepInputOutcome::Rejected(error) => {
            fail_run(run, execution, port, error.to_string()).await?;
            return Ok(());
        }
    };
    emit_events(run, execution, port).await?;
    // -- compaction check --
    let needs_compaction =
        match await_interruptible(run, &step_cancel, port.needs_compaction(execution)).await {
            Interrupt::Completed(result) => result?,
            Interrupt::Cancelled => {
                handle_step_control(run, execution, port).await?;
                return Ok(());
            }
            Interrupt::TimedOut => {
                timeout_run(run, execution, port).await?;
                return Ok(());
            }
        };
    if needs_compaction {
        run.transition(RunTransition::BeginCompaction)?;
        match run_context_compaction_phase(run, execution, &step_cancel, port.compaction_mut())
            .await?
        {
            ContextCompactionOutcome::Ready => {}
            ContextCompactionOutcome::Cancelled => {
                return handle_step_control(run, execution, port).await;
            }
            ContextCompactionOutcome::TimedOut => {
                timeout_run(run, execution, port).await?;
                return Ok(());
            }
        }
        run.transition(RunTransition::CompactionCompleted)?;
    }

    if handle_interrupt(run, execution, cancel, port).await? {
        return Ok(());
    }
    run.transition(RunTransition::ContextPrepared)?;
    let mut compacted_after_context_too_long = false;
    let (model_step, token_usage) = loop {
        match run_model_invocation_phase(run, execution, &step_cancel, port.model_mut()).await {
            ModelInvocationOutcome::Invoked(step, usage) => break (step, usage),
            ModelInvocationOutcome::Cancelled => {
                handle_step_control(run, execution, port).await?;
                return Ok(());
            }
            ModelInvocationOutcome::NeedsCompaction(error) => {
                if compacted_after_context_too_long {
                    fail_run(
                        run,
                        execution,
                        port,
                        format!("compact 后 Provider 仍报告 context 超限：{error}"),
                    )
                    .await?;
                    return Ok(());
                }
                run.transition(RunTransition::ModelContextExceeded)?;
                match run_context_compaction_phase(
                    run,
                    execution,
                    &step_cancel,
                    port.compaction_mut(),
                )
                .await?
                {
                    ContextCompactionOutcome::Ready => {}
                    ContextCompactionOutcome::Cancelled => {
                        handle_step_control(run, execution, port).await?;
                        return Ok(());
                    }
                    ContextCompactionOutcome::TimedOut => {
                        timeout_run(run, execution, port).await?;
                        return Ok(());
                    }
                }
                run.transition(RunTransition::CompactionCompleted)?;
                run.transition(RunTransition::ContextPrepared)?;
                compacted_after_context_too_long = true;
            }
            ModelInvocationOutcome::Failed(error) => {
                fail_run(run, execution, port, error.to_string()).await?;
                return Ok(());
            }
            ModelInvocationOutcome::TimedOut => {
                timeout_run(run, execution, port).await?;
                return Ok(());
            }
        }
    };

    // Per-step token usage + context window 诊断日志
    {
        let ctx_win = token_usage.context_window;
        let total = token_usage.total_tokens;
        let pct = total
            .checked_mul(100)
            .and_then(|v| v.checked_div(ctx_win))
            .map(|v| v as u32)
            .unwrap_or(0);
        log::info!(
            target: crate::LOG_TARGET,
            "token usage: input={} (cached {}) | output={} (cache_write {}) | reasoning={} | total={} | context_window={} | {pct}% \
             | stop_reason={} | est: system={} tools={} messages={} total_est={}",
            token_usage.input_tokens,
            token_usage.cached_tokens,
            token_usage.output_tokens,
            token_usage.cache_creation_tokens,
            token_usage.reasoning_tokens,
            total,
            ctx_win,
            token_usage.stop_reason,
            token_usage.est_system_tokens,
            token_usage.est_tool_tokens,
            token_usage.est_message_tokens,
            token_usage.est_total(),
        );
    }
    if handle_interrupt(run, execution, cancel, port).await? {
        return Ok(());
    }
    run.record_model_invocation(&step_id, model_invocation(&model_step))?;
    run.transition(RunTransition::ModelInvoked)?;
    log::debug!(
        target: crate::LOG_TARGET,
        "[run_loop] model_step={} run_id={}",
        model_step_label(&model_step),
        short(run.id()),
    );

    // #1272: track the last assistant text for terminal claim
    let assistant_text = model_step_text(&model_step);
    *terminal_text = Some(assistant_text);

    match model_step {
        ModelStep::Complete { text } => {
            // Text-only completion is handled by the static reasoning level.

            // #1248 Task 5: Plan approval before proceeding when in plan mode.
            if port.needs_plan_approval() && !text.trim().is_empty() {
                handle_plan_approval(run, execution, port, &step_id, &text).await?;
                return Ok(());
            }

            // #1248 Task 6: Evaluate Stop hook BEFORE text stall check.
            // A blocking stop hook with repeated output should continue
            // (feedback may change the model's behavior); text stall
            // detection runs only when the stop hook allows proceeding.
            let stop_outcome = port
                .coordinate_stop_hook(execution, run.steps().len(), &step_cancel)
                .await?;
            match stop_outcome.decision {
                StopHookDecision::Proceed => {
                    // Normal completion — fall through to text stall check.
                }
                StopHookDecision::Block(ref block) => {
                    let block_result = run.record_stop_hook_block();
                    log::info!(
                        target: crate::LOG_TARGET,
                        "[stop_hook] blocked: reason={:?} count={}",
                        block.reason,
                        run.stop_hook_block_count(),
                    );
                    match block_result {
                        StopHookBlockResult::Blocked { .. } => {
                            // #1272: Block → ContinueAfterResponse for another attempt.
                            run.transition(RunTransition::ContinueAfterResponse)?;
                            run.complete_step(&step_id)?;
                            run_step_finalization_phase(
                                execution,
                                port.persistence_mut(),
                                &step_id,
                                crate::ports::FinalizeCause::Completed,
                            )
                            .await?;
                            return Ok(());
                        }
                        StopHookBlockResult::RetryExhausted { count } => {
                            // 16th block → Run Failed.
                            fail_run(
                                run,
                                execution,
                                port,
                                format!(
                                    "stop hook blocked completion {count} times (retry exhausted)"
                                ),
                            )
                            .await?;
                            return Ok(());
                        }
                    }
                }
            }

            // Text stall detection only after stop hook allows proceeding.
            match guard.inspect_text(terminal_text.as_deref().unwrap_or("")) {
                decision @ StuckDecision::SoftBlock { .. } => {
                    record_stuck(run, execution, port, &decision).await?;
                    run.transition(RunTransition::ContinueAfterResponse)?;
                    run.complete_step(&step_id)?;
                    run_step_finalization_phase(
                        execution,
                        port.persistence_mut(),
                        &step_id,
                        crate::ports::FinalizeCause::Completed,
                    )
                    .await?;
                    return Ok(());
                }
                decision @ StuckDecision::HardPause { .. } => {
                    let reason = match &decision {
                        StuckDecision::HardPause { reason } => reason.clone(),
                        _ => unreachable!(),
                    };
                    record_stuck(run, execution, port, &decision).await?;
                    handle_hard_pause(run, execution, port, &step_id, reason).await?;
                    return Ok(());
                }
                StuckDecision::Allow => {}
            }

            // #1272: Complete goes to DrainingInput (not Finishing→Finish)
            run.transition(RunTransition::ContinueAfterResponse)?;
            run.complete_step(&step_id)?;
            run_step_finalization_phase(
                execution,
                port.persistence_mut(),
                &step_id,
                crate::ports::FinalizeCause::Completed,
            )
            .await?;
            // Loop back to drain — adapter returns EmptyAndSealed for Complete
        }
        #[cfg(test)]
        ModelStep::Continue { text: _ } => {
            let decision = guard.inspect_text(terminal_text.as_deref().unwrap_or(""));
            match decision {
                StuckDecision::SoftBlock { .. } => {
                    record_stuck(run, execution, port, &decision).await?
                }
                StuckDecision::HardPause { ref reason } => {
                    let reason = reason.clone();
                    record_stuck(run, execution, port, &decision).await?;
                    handle_hard_pause(run, execution, port, &step_id, reason).await?;
                    return Ok(());
                }
                StuckDecision::Allow => {}
            }
            // #1272: Continue goes to DrainingInput
            run.transition(RunTransition::ContinueAfterResponse)?;
            run.complete_step(&step_id)?;
            run_step_finalization_phase(
                execution,
                port.persistence_mut(),
                &step_id,
                crate::ports::FinalizeCause::Completed,
            )
            .await?;
        }
        ModelStep::Tools { text: _, calls } => {
            if let decision @ StuckDecision::SoftBlock { .. } =
                guard.inspect_text(terminal_text.as_deref().unwrap_or(""))
            {
                record_stuck(run, execution, port, &decision).await?;
            }
            run.transition(RunTransition::ResponseWithTools)?;
            let mut guarded_calls = Vec::with_capacity(calls.len());
            for call in calls {
                run.add_tool_call(&step_id, call.clone())?;
                match guard.inspect_tool(&call) {
                    StuckDecision::SoftBlock { reason } => {
                        record_stuck(
                            run,
                            execution,
                            port,
                            &StuckDecision::SoftBlock {
                                reason: reason.clone(),
                            },
                        )
                        .await?;
                        guarded_calls.push((call, ToolGuardDecision::SoftBlock { reason }));
                    }
                    StuckDecision::HardPause { reason } => {
                        record_stuck(
                            run,
                            execution,
                            port,
                            &StuckDecision::HardPause {
                                reason: reason.clone(),
                            },
                        )
                        .await?;
                        handle_hard_pause(run, execution, port, &step_id, reason).await?;
                        return Ok(());
                    }
                    StuckDecision::Allow => {
                        guarded_calls.push((call, ToolGuardDecision::Allow));
                    }
                }
            }
            for (call, _) in &guarded_calls {
                run.advance_tool_call(&step_id, &call.id, ToolCallStatus::Ready)?;
            }
            run.transition(RunTransition::ToolsApproved)?;
            for (call, decision) in &guarded_calls {
                if matches!(decision, ToolGuardDecision::Allow) {
                    run.advance_tool_call(&step_id, &call.id, ToolCallStatus::Running)?;
                }
            }
            log::debug!(
                target: crate::LOG_TARGET,
                "[run_loop] execute_tools count={} run_id={}",
                guarded_calls.len(),
                short(run.id()),
            );
            let tool_outcome = match run_tool_round_phase(
                run,
                execution,
                run.id(),
                &step_id,
                &guarded_calls,
                &step_cancel,
                port.tools_mut(),
            )
            .await
            {
                ToolRoundPhaseOutcome::Completed(outcome) => outcome,
                ToolRoundPhaseOutcome::Cancelled => {
                    handle_step_control(run, execution, port).await?;
                    return Ok(());
                }
                ToolRoundPhaseOutcome::Failed(error) => {
                    fail_run(run, execution, port, error.to_string()).await?;
                    return Ok(());
                }
                ToolRoundPhaseOutcome::TimedOut => {
                    timeout_run(run, execution, port).await?;
                    return Ok(());
                }
            };
            if handle_interrupt(run, execution, cancel, port).await? {
                return Ok(());
            }
            if matches!(
                tool_outcome.continuation,
                crate::application::tool::coordination::ToolRoundContinuation::ToolResults
            ) {
                port.schedule_internal_continuation(InternalContinuationKind::ToolResults);
            }
            let tool_step = tool_outcome.step;
            let (fuse_bypassed, completed_non_interaction): (
                &[sdk::ToolCallId],
                &[(sdk::ToolCallId, ToolCallStatus)],
            ) = match &tool_step {
                ToolStep::ContinueWithFuseBypass(ids) => (ids.as_slice(), &[]),
                ToolStep::InteractionSuspended {
                    fuse_bypassed,
                    completed_results,
                    ..
                }
                | ToolStep::AwaitingToolApproval {
                    fuse_bypassed,
                    completed_results,
                    ..
                } => (fuse_bypassed.as_slice(), completed_results.as_slice()),
                ToolStep::Continue => (&[], &[]),
                #[cfg(test)]
                ToolStep::AwaitUser => (&[], &[]),
            };
            // #1248: For interaction steps, advance completed (non-interaction) results
            // before the interaction coordinator takes over.  Interaction calls are NOT
            // advanced here — the interaction path manages them via `advance_tool_call`.
            if !completed_non_interaction.is_empty() {
                for (call_id, status) in completed_non_interaction {
                    run.advance_tool_call(&step_id, call_id, *status)?;
                }
            }
            // Only advance tool calls for non-interaction steps.
            // Interaction paths manage tool call status themselves.
            let is_interaction = matches!(
                &tool_step,
                ToolStep::InteractionSuspended { .. } | ToolStep::AwaitingToolApproval { .. }
            );
            if !is_interaction {
                for (call, decision) in &guarded_calls {
                    let bypassed = fuse_bypassed.contains(&call.id);
                    let status = if matches!(decision, ToolGuardDecision::Allow) || bypassed {
                        ToolCallStatus::Success
                    } else {
                        ToolCallStatus::Cancelled
                    };
                    run.advance_tool_call(&step_id, &call.id, status)?;
                }
            }
            match tool_step {
                ToolStep::Continue | ToolStep::ContinueWithFuseBypass(_) => {
                    run.complete_step(&step_id)?;
                    run_step_finalization_phase(
                        execution,
                        port.persistence_mut(),
                        &step_id,
                        crate::ports::FinalizeCause::Completed,
                    )
                    .await?;
                    // #1272: ToolsCompleted → DrainingInput (not PreparingContext)
                    run.transition(RunTransition::ToolsCompleted)?;
                }
                #[cfg(test)]
                ToolStep::AwaitUser => {
                    run.complete_step(&step_id)?;
                    // AwaitUser 前必须先 finalize step outcome（模型回复 +
                    // 工具结果），否则 Terminate 时 active_step 为 None，
                    // 上一 step 的 outcome 永久丢失。
                    run_step_finalization_phase(
                        execution,
                        port.persistence_mut(),
                        &step_id,
                        crate::ports::FinalizeCause::Completed,
                    )
                    .await?;
                    run.transition(RunTransition::AwaitUser)?;
                    emit_events(run, execution, port).await?;
                    // Return to caller; the caller will call run_loop again
                    // with drain_input picking up the user response.
                    return Ok(());
                }
                ToolStep::InteractionSuspended { suspended, .. } => {
                    // #1248 Task 5: Resolve suspensions through coordinator
                    handle_suspensions(run, execution, port, suspended).await?;
                }
                ToolStep::AwaitingToolApproval {
                    calls_needing_approval,
                    ..
                } => {
                    // #1248 Task 5: Resolve tool approvals through coordinator
                    handle_tool_approvals(run, execution, port, calls_needing_approval).await?;
                }
            }
        }
    }
    Ok(())
}

/// Extract assistant text from a model step for terminal tracking.
fn model_step_text(step: &ModelStep) -> String {
    match step {
        ModelStep::Complete { text } | ModelStep::Tools { text, .. } => text.clone(),
        #[cfg(test)]
        ModelStep::Continue { text } => text.clone(),
    }
}

fn model_invocation(step: &ModelStep) -> ModelInvocation {
    let response = match step {
        ModelStep::Complete { text } | ModelStep::Tools { text, .. } => text.clone(),
        #[cfg(test)]
        ModelStep::Continue { text } => text.clone(),
    };
    ModelInvocation::new(response)
}

/// #1248 Task 5: Resolve tool suspensions through the interaction coordinator.
/// Creates `UserQuestions` intents, registers them via coordinator, publishes
/// to UI, stores the receiver on the port, and leaves the Run in AwaitingUser.
/// The actual reply/cancel is handled on the next drain cycle via
/// `poll_interaction`.
async fn handle_suspensions(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    suspended: Vec<SuspendedToolCall>,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;

    // #1248 Task 5: Start ONLY the first suspension. Queue the rest on the port
    // so they are handled one-at-a-time via finish_interaction_work.
    let mut iter = suspended.into_iter();
    let first = match iter.next() {
        Some(first) => first,
        None => return Ok(()),
    };
    let remaining: Vec<_> = iter.collect();

    // Build the current item for the first interaction — stores the full
    // SuspendedToolCall so finish_interaction_work can use the original
    // call's provider_id/name.
    let first_request_id = sdk::InteractionRequestId::new_v7();
    let first_continuation = InteractionContinuation::CompleteToolCall(first.call.id.clone());
    let current_item = PendingInteractionItem {
        request_id: first_request_id.clone(),
        continuation: first_continuation.clone(),
        suspended_call: Some(first.clone()),
        approval_call: None,
    };

    if !remaining.is_empty() {
        // Queue remaining on the port — the engine will start the next one
        // after the first resolves via finish_interaction_work.
        let queue: Vec<PendingInteractionItem> = remaining
            .into_iter()
            .map(|sc| {
                let request_id = sdk::InteractionRequestId::new_v7();
                PendingInteractionItem {
                    request_id,
                    continuation: InteractionContinuation::CompleteToolCall(sc.call.id.clone()),
                    suspended_call: Some(sc),
                    approval_call: None,
                }
            })
            .collect();
        port.set_pending_interaction_work(
            execution,
            PendingInteractionWork {
                current: Some(current_item.clone()),
                queue,
            },
        );
    } else {
        // No remaining items — still store the current item so
        // finish_interaction_work can look up the original call.
        port.set_pending_interaction_work(
            execution,
            PendingInteractionWork {
                current: Some(current_item.clone()),
                queue: Vec::new(),
            },
        );
    }

    // Start the first interaction
    {
        let questions: Vec<sdk::UserQuestion> = first
            .questions
            .iter()
            .map(|q| sdk::UserQuestion {
                prompt: q.prompt.clone(),
                options: q.options.clone(),
                allow_multi: q.allow_multi,
            })
            .collect();
        let body = sdk::InteractionRequestBody::UserQuestions(questions);
        let run_id = run.id().clone();

        let (rid, receiver) = InteractionCoordinator::begin(
            run,
            port.interaction_port(),
            first_request_id.clone(),
            run_id.clone(),
            body.clone(),
            first_continuation.clone(),
        )
        .map_err(|e| LoopEngineError::Adapter(format!("interaction begin failed: {e:?}")))?;

        // Publish to UI
        let request = sdk::InteractionRequest {
            id: first_request_id.clone(),
            run_id: run_id.clone(),
            tool_call_id: Some(first.call.id.to_string()),
            body: body.clone(),
        };
        port.publish_interaction(execution, &request).await?;

        let metadata = crate::application::interaction::port::InteractionRequestMetadata::new(
            first_request_id,
            run_id,
            body,
            first_continuation.clone(),
        );
        crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
            execution, metadata, receiver,
        )
        .map_err(|error| {
            LoopEngineError::Adapter(format!(
                "interaction mailbox registration failed: {error:?}"
            ))
        })?;

        log::debug!(
            target: crate::LOG_TARGET,
            "[handle_suspensions] registered first interaction rid={:?} call={}",
            rid,
            first.call.id,
        );
    }

    // #1248: Do NOT advance tool calls to Success here — the calls are NOT
    // resolved yet.  Do NOT complete_step — the step stays active until all
    // interactions resolve via finish_interaction_work → advance_tool_call.
    // Only emit events so the UI knows the run is awaiting user input.
    emit_events(run, execution, port).await?;
    Ok(())
}

/// #1248 Task 5: Resolve tool approvals through the interaction coordinator.
/// Creates `ToolApproval` intents — only starts the FIRST, queues remaining.
async fn handle_tool_approvals(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    calls_needing_approval: Vec<ApprovalRequiredCall>,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;

    // Only start the FIRST approval; queue the rest.
    let mut iter = calls_needing_approval.into_iter();
    let first = match iter.next() {
        Some(first) => first,
        None => return Ok(()),
    };
    let remaining: Vec<_> = iter.collect();

    // Build the current item for the first approval — stores the full
    // ApprovalRequiredCall so finish_interaction_work can execute directly.
    let first_request_id = sdk::InteractionRequestId::new_v7();
    let first_continuation = InteractionContinuation::ContinueToolApproval(first.call.id.clone());
    let current_item = PendingInteractionItem {
        request_id: first_request_id.clone(),
        continuation: first_continuation.clone(),
        suspended_call: None,
        approval_call: Some(first.clone()),
    };

    if !remaining.is_empty() {
        let queue: Vec<PendingInteractionItem> = remaining
            .into_iter()
            .map(|ac| {
                let request_id = sdk::InteractionRequestId::new_v7();
                PendingInteractionItem {
                    request_id,
                    continuation: InteractionContinuation::ContinueToolApproval(ac.call.id.clone()),
                    suspended_call: None,
                    approval_call: Some(ac),
                }
            })
            .collect();
        port.set_pending_interaction_work(
            execution,
            PendingInteractionWork {
                current: Some(current_item.clone()),
                queue,
            },
        );
    } else {
        port.set_pending_interaction_work(
            execution,
            PendingInteractionWork {
                current: Some(current_item.clone()),
                queue: Vec::new(),
            },
        );
    }

    // Start the first approval
    {
        let body = sdk::InteractionRequestBody::ToolApproval(sdk::ToolApprovalPrompt {
            tool_name: first.call.name.clone(),
            args_summary: first.reason.clone(),
            risk_level: sdk::RiskLevel::Medium,
        });
        let run_id = run.id().clone();

        let (_rid, receiver) = InteractionCoordinator::begin(
            run,
            port.interaction_port(),
            first_request_id.clone(),
            run_id.clone(),
            body.clone(),
            first_continuation.clone(),
        )
        .map_err(|e| LoopEngineError::Adapter(format!("tool approval begin failed: {e:?}")))?;

        let request = sdk::InteractionRequest {
            id: first_request_id.clone(),
            run_id: run_id.clone(),
            tool_call_id: None,
            body: body.clone(),
        };
        port.publish_interaction(execution, &request).await?;
        let metadata = crate::application::interaction::port::InteractionRequestMetadata::new(
            first_request_id.clone(),
            run_id.clone(),
            body.clone(),
            first_continuation.clone(),
        );
        crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
            execution, metadata, receiver,
        )
        .map_err(|error| {
            LoopEngineError::Adapter(format!(
                "interaction mailbox registration failed: {error:?}"
            ))
        })?;
    }

    emit_events(run, execution, port).await?;
    Ok(())
}

/// #1248: Handle a resolved or closed interaction via the coordinator.
/// After validation, calls `finish_interaction_work` to write results,
/// advances tool calls, and either starts the next queued interaction
/// or completes the step.
async fn handle_interaction_completion(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    step_cancel: &CancellationToken,
    resolution: crate::application::interaction::port::InteractionResolution,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;
    use crate::application::interaction::port::InteractionCompletion;
    use crate::application::interaction::port::InteractionResolution;

    let metadata = resolution.metadata().clone();
    let request_id = metadata.request_id.clone();
    let run_id = metadata.run_id.clone();

    match &resolution {
        InteractionResolution::Resolved {
            completion: InteractionCompletion::Replied(reply),
            ..
        } => {
            log::debug!(
                target: crate::LOG_TARGET,
                "[handle_interaction_completion] replied request={request_id}",
            );
            // Validate and complete via coordinator
            let continuation =
                InteractionCoordinator::complete_reply(run, &request_id, &metadata.body, reply)
                    .map_err(|e| {
                        log::error!(target: crate::LOG_TARGET, "complete_reply failed: {e:?}");
                        LoopEngineError::Adapter(format!("interaction completion failed: {e:?}"))
                    })?;

            // Dispatch based on the continuation type
            dispatch_continuation(
                run,
                execution,
                port,
                step_cancel,
                &metadata,
                &continuation,
                reply,
            )
            .await?;
        }
        InteractionResolution::Resolved {
            completion: InteractionCompletion::Cancelled(reason),
            ..
        } => {
            log::debug!(
                target: crate::LOG_TARGET,
                "[handle_interaction_completion] cancelled request={request_id} reason={reason:?}",
            );
            let _ = InteractionCoordinator::cancel(run, &request_id);

            let outcome = {
                let context = port.interaction_completion_context(step_cancel.clone());
                InteractionCoordinator::complete_tool_interaction(
                    &context,
                    execution,
                    &metadata,
                    &InteractionCompletion::Cancelled(reason.clone()),
                )
                .await?
            };
            handle_interaction_outcome(run, execution, port, step_cancel, outcome).await?;
        }
        InteractionResolution::Closed { .. } => {
            log::warn!(
                target: crate::LOG_TARGET,
                "[handle_interaction_completion] closed request={request_id}",
            );
            let _ = InteractionCoordinator::cancel_and_drain(
                run,
                execution,
                port.interaction_port(),
                &run_id,
                sdk::InteractionCancelReason::RunCancelled,
            );
            let outcome = {
                let context = port.interaction_completion_context(step_cancel.clone());
                InteractionCoordinator::complete_tool_interaction(
                    &context,
                    execution,
                    &metadata,
                    &InteractionCompletion::Cancelled(sdk::InteractionCancelReason::RunCancelled),
                )
                .await?
            };
            handle_interaction_outcome(run, execution, port, step_cancel, outcome).await?;
        }
    }

    emit_events(run, execution, port).await?;
    Ok(())
}

/// #1248: Dispatch after a completed interaction based on the continuation type.
async fn dispatch_continuation(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    step_cancel: &CancellationToken,
    metadata: &crate::application::interaction::port::InteractionRequestMetadata,
    continuation: &InteractionContinuation,
    reply: &sdk::InteractionReply,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;
    use crate::application::interaction::port::InteractionCompletion;

    match continuation {
        InteractionContinuation::CompleteToolCall(tool_call_id) => {
            let outcome = {
                let context = port.interaction_completion_context(step_cancel.clone());
                InteractionCoordinator::complete_tool_interaction(
                    &context,
                    execution,
                    metadata,
                    &InteractionCompletion::Replied(reply.clone()),
                )
                .await?
            };
            handle_interaction_outcome(run, execution, port, step_cancel, outcome).await?;
            // Suppress unused warning
            let _ = tool_call_id;
        }
        InteractionContinuation::ContinueAfterHardPause => {
            // HardPauseContinue: the continuation already transitioned
            // the run back to ExecutingTools via complete_interaction.
            log::debug!(
                target: crate::LOG_TARGET,
                "[dispatch_continuation] HardPauseContinue — resuming"
            );
        }
        InteractionContinuation::ContinuePlanApproval => {
            if matches!(
                reply,
                sdk::InteractionReply::PlanApproval(sdk::ApprovalDecision::Deny { .. })
            ) {
                let _ = InteractionCoordinator::cancel_and_drain(
                    run,
                    execution,
                    port.interaction_port(),
                    &metadata.run_id,
                    sdk::InteractionCancelReason::UserCancelled,
                );
                emit_events(run, execution, port).await?;
            }
            // On approve: complete_interaction already transitioned back;
            // the step was already completed in handle_plan_approval.
        }
        InteractionContinuation::ContinueToolApproval(tool_call_id) => {
            if matches!(
                reply,
                sdk::InteractionReply::ToolApproval(sdk::ApprovalDecision::Approve)
            ) {
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[dispatch_continuation] ToolApproval approve for call={tool_call_id}"
                );
                let outcome = {
                    let context = port.interaction_completion_context(step_cancel.clone());
                    InteractionCoordinator::complete_tool_interaction(
                        &context,
                        execution,
                        metadata,
                        &InteractionCompletion::Replied(reply.clone()),
                    )
                    .await?
                };
                handle_interaction_outcome(run, execution, port, step_cancel, outcome).await?;
            } else {
                let outcome = {
                    let context = port.interaction_completion_context(step_cancel.clone());
                    InteractionCoordinator::complete_tool_interaction(
                        &context,
                        execution,
                        metadata,
                        &InteractionCompletion::Cancelled(
                            sdk::InteractionCancelReason::UserCancelled,
                        ),
                    )
                    .await?
                };
                handle_interaction_outcome(run, execution, port, step_cancel, outcome).await?;
            }
        }
    }

    Ok(())
}

/// #1248: Process the outcome of `finish_interaction_work`.
/// Advances the resolved tool call, then either starts the next queued
/// interaction or completes the step.
async fn handle_interaction_outcome(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    _step_cancel: &CancellationToken,
    outcome: InteractionWorkOutcome,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;

    match outcome {
        InteractionWorkOutcome::Completed {
            call_id,
            status,
            remaining_queue,
            schedule_tool_results,
        } => {
            if schedule_tool_results {
                port.schedule_internal_continuation(InternalContinuationKind::ToolResults);
            }
            // Advance the just-resolved tool call — find the step
            let step_id = run
                .active_step_id()
                .ok_or_else(|| LoopEngineError::Adapter("no active step".to_string()))?;
            run.advance_tool_call(&step_id, &call_id, status)?;

            if remaining_queue.is_empty() {
                // All interactions resolved — complete the step
                run.complete_step(&step_id)?;
                run_step_finalization_phase(
                    execution,
                    port.persistence_mut(),
                    &step_id,
                    crate::ports::FinalizeCause::Completed,
                )
                .await?;
                run.transition(RunTransition::ToolsCompleted)?;
            } else {
                // Start the next interaction from the queue
                let next = remaining_queue[0].clone();
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[handle_interaction_outcome] starting next interaction rid={:?}",
                    next.request_id,
                );

                if let Some(ref suspended) = next.suspended_call {
                    let questions: Vec<sdk::UserQuestion> = suspended
                        .questions
                        .iter()
                        .map(|q| sdk::UserQuestion {
                            prompt: q.prompt.clone(),
                            options: q.options.clone(),
                            allow_multi: q.allow_multi,
                        })
                        .collect();
                    let body = sdk::InteractionRequestBody::UserQuestions(questions);
                    let run_id = run.id().clone();

                    let (rid, receiver) = InteractionCoordinator::begin(
                        run,
                        port.interaction_port(),
                        next.request_id.clone(),
                        run_id.clone(),
                        body.clone(),
                        next.continuation.clone(),
                    )
                    .map_err(|e| {
                        LoopEngineError::Adapter(format!("queued interaction begin failed: {e:?}"))
                    })?;

                    let request = sdk::InteractionRequest {
                        id: next.request_id.clone(),
                        run_id: run_id.clone(),
                        tool_call_id: Some(suspended.call.id.to_string()),
                        body: body.clone(),
                    };
                    port.publish_interaction(execution, &request).await?;
                    let metadata =
                        crate::application::interaction::port::InteractionRequestMetadata::new(
                            next.request_id.clone(),
                            run_id,
                            body,
                            next.continuation.clone(),
                        );
                    crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
            execution, metadata, receiver,
        )
        .map_err(|error| {
            LoopEngineError::Adapter(format!(
                "interaction mailbox registration failed: {error:?}"
            ))
        })?;
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[handle_interaction_outcome] queued UserQuestions rid={:?}",
                        rid,
                    );
                } else if let Some(ref approval) = next.approval_call {
                    let body = sdk::InteractionRequestBody::ToolApproval(sdk::ToolApprovalPrompt {
                        tool_name: approval.call.name.clone(),
                        args_summary: approval.reason.clone(),
                        risk_level: sdk::RiskLevel::Medium,
                    });
                    let run_id = run.id().clone();

                    let (_rid, receiver) = InteractionCoordinator::begin(
                        run,
                        port.interaction_port(),
                        next.request_id.clone(),
                        run_id.clone(),
                        body.clone(),
                        next.continuation.clone(),
                    )
                    .map_err(|e| {
                        LoopEngineError::Adapter(format!(
                            "queued tool approval begin failed: {e:?}"
                        ))
                    })?;

                    let request = sdk::InteractionRequest {
                        id: next.request_id.clone(),
                        run_id: run_id.clone(),
                        tool_call_id: None,
                        body: body.clone(),
                    };
                    port.publish_interaction(execution, &request).await?;
                    let metadata =
                        crate::application::interaction::port::InteractionRequestMetadata::new(
                            next.request_id.clone(),
                            run_id,
                            body,
                            next.continuation.clone(),
                        );
                    crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
            execution, metadata, receiver,
        )
        .map_err(|error| {
            LoopEngineError::Adapter(format!(
                "interaction mailbox registration failed: {error:?}"
            ))
        })?;
                }

                // Update the port: new current = the now-started item,
                // new queue = items after it.
                let rest: Vec<PendingInteractionItem> = if remaining_queue.len() > 1 {
                    remaining_queue[1..].to_vec()
                } else {
                    Vec::new()
                };
                port.set_pending_interaction_work(
                    execution,
                    PendingInteractionWork {
                        current: Some(next),
                        queue: rest,
                    },
                );
            }
        }
    }

    Ok(())
}

/// #1248 Task 5: Handle a HardPause stuck decision via the interaction coordinator.
/// Instead of failing the run, creates a HardPause interaction request that the
/// user can continue from. On resume, the run transitions back to ExecutingTools.
async fn handle_hard_pause(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    step_id: &sdk::RunStepId,
    reason: String,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;

    let request_id = sdk::InteractionRequestId::new_v7();
    let body = sdk::InteractionRequestBody::HardPause(sdk::StuckDiagnostic {
        reason: reason.clone(),
        recent_actions: vec![],
    });
    let continuation = InteractionContinuation::ContinueAfterHardPause;
    let run_id = run.id().clone();

    let (_rid, receiver) = InteractionCoordinator::begin(
        run,
        port.interaction_port(),
        request_id.clone(),
        run_id.clone(),
        body.clone(),
        continuation.clone(),
    )
    .map_err(|e| {
        // If interaction is unavailable, fall back to failing the run.
        log::error!(target: crate::LOG_TARGET, "HardPause interaction begin failed: {e:?}");
        LoopEngineError::Adapter(format!("HardPause interaction unavailable: {e:?}"))
    })?;

    // Publish to UI and store receiver
    let request = sdk::InteractionRequest {
        id: request_id.clone(),
        run_id: run_id.clone(),
        tool_call_id: None,
        body: body.clone(),
    };
    port.publish_interaction(execution, &request).await?;
    let metadata = crate::application::interaction::port::InteractionRequestMetadata::new(
        request_id.clone(),
        run_id.clone(),
        body.clone(),
        continuation.clone(),
    );
    crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
        execution, metadata, receiver,
    )
    .map_err(|error| {
        LoopEngineError::Adapter(format!(
            "interaction mailbox registration failed: {error:?}"
        ))
    })?;

    // Complete step and transition to AwaitingUser
    run.complete_step(step_id)?;
    run_step_finalization_phase(
        execution,
        port.persistence_mut(),
        step_id,
        crate::ports::FinalizeCause::Completed,
    )
    .await?;
    emit_events(run, execution, port).await?;

    Ok(())
}

/// #1248 Task 5: Handle plan approval via the interaction coordinator.
/// When the model produces a Complete response in plan mode, the user must review
/// the plan before the run proceeds. On approve, the run continues; on reject,
/// the run is cancelled.
async fn handle_plan_approval(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    step_id: &sdk::RunStepId,
    plan_text: &str,
) -> Result<(), LoopEngineError> {
    use crate::application::interaction::coordinator::InteractionCoordinator;

    let request_id = sdk::InteractionRequestId::new_v7();
    let body = sdk::InteractionRequestBody::PlanApproval(sdk::PlanApprovalPrompt {
        plan_title: String::new(),
        steps: vec![plan_text.to_string()],
    });
    let continuation = InteractionContinuation::ContinuePlanApproval;
    let run_id = run.id().clone();

    let (_rid, receiver) = InteractionCoordinator::begin(
        run,
        port.interaction_port(),
        request_id.clone(),
        run_id.clone(),
        body.clone(),
        continuation.clone(),
    )
    .map_err(|e| {
        log::error!(target: crate::LOG_TARGET, "PlanApproval interaction begin failed: {e:?}");
        LoopEngineError::Adapter(format!("PlanApproval interaction unavailable: {e:?}"))
    })?;

    let request = sdk::InteractionRequest {
        id: request_id.clone(),
        run_id: run_id.clone(),
        tool_call_id: None,
        body: body.clone(),
    };
    port.publish_interaction(execution, &request).await?;
    let metadata = crate::application::interaction::port::InteractionRequestMetadata::new(
        request_id.clone(),
        run_id.clone(),
        body.clone(),
        continuation.clone(),
    );
    crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
        execution, metadata, receiver,
    )
    .map_err(|error| {
        LoopEngineError::Adapter(format!(
            "interaction mailbox registration failed: {error:?}"
        ))
    })?;

    run.complete_step(step_id)?;
    run_step_finalization_phase(
        execution,
        port.persistence_mut(),
        step_id,
        crate::ports::FinalizeCause::Completed,
    )
    .await?;
    emit_events(run, execution, port).await?;

    Ok(())
}

async fn record_stuck(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    decision: &StuckDecision,
) -> Result<(), LoopEngineError> {
    let reason = match decision {
        StuckDecision::SoftBlock { reason } | StuckDecision::HardPause { reason } => reason.clone(),
        StuckDecision::Allow => return Ok(()),
    };
    run.mark_stuck(reason)?;
    emit_events(run, execution, port).await?;
    port.on_stuck(execution, decision).await
}

enum ControlDirective {
    Continue,
    Terminal,
}

async fn handle_pending_control(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
) -> Result<Option<ControlDirective>, LoopEngineError> {
    let Some(control) = port.take_control(run.id()) else {
        return Ok(None);
    };
    let active_step = run.active_step_id();
    match control {
        crate::domain::agent_run::RunControl::CancelStep { step_id, .. } => {
            if active_step.as_ref() != Some(&step_id) {
                return Err(LoopEngineError::Adapter(
                    "CancelRunStep 与当前 Step identity 不匹配".to_string(),
                ));
            }
            finish_cancelled_step(run, execution, port, &step_id).await?;
            Ok(Some(ControlDirective::Continue))
        }
        crate::domain::agent_run::RunControl::Terminate { reason, deadline } => {
            match run.request_termination(reason, deadline) {
                crate::domain::agent_run::RunTerminationRequest::Accepted => {}
                crate::domain::agent_run::RunTerminationRequest::AlreadyTerminating
                | crate::domain::agent_run::RunTerminationRequest::AlreadyTerminal => {
                    return Ok(Some(ControlDirective::Terminal));
                }
            }
            emit_events(run, execution, port).await?;
            if let Some(step_id) = active_step {
                run_step_finalization_phase(
                    execution,
                    port.persistence_mut(),
                    &step_id,
                    crate::ports::FinalizeCause::UserCancelledStep,
                )
                .await?;
            }
            run.finish_termination()?;
            emit_events(run, execution, port).await?;
            Ok(Some(ControlDirective::Terminal))
        }
    }
}

async fn finish_cancelled_step(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    step_id: &sdk::RunStepId,
) -> Result<(), LoopEngineError> {
    match run.request_step_cancellation(step_id) {
        crate::domain::agent_run::RunStepCancellationRequest::Accepted => {}
        crate::domain::agent_run::RunStepCancellationRequest::AlreadyCancelling => return Ok(()),
        outcome => {
            return Err(LoopEngineError::Adapter(format!(
                "取消当前 Step 时获得了非预期结果：{outcome:?}"
            )));
        }
    }
    emit_events(run, execution, port).await?;
    run.begin_step_finalization(step_id)?;
    emit_events(run, execution, port).await?;
    run_step_finalization_phase(
        execution,
        port.persistence_mut(),
        step_id,
        crate::ports::FinalizeCause::UserCancelledStep,
    )
    .await?;
    run.finish_cancelled_step(step_id)?;
    port.clear_cancelled_step_scope(run.id(), step_id);
    emit_events(run, execution, port).await
}

async fn handle_step_control(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
) -> Result<(), LoopEngineError> {
    match handle_pending_control(run, execution, port).await? {
        Some(ControlDirective::Continue) => Ok(()),
        Some(ControlDirective::Terminal) => Ok(()),
        None => {
            cancel_run(run, execution, port).await?;
            Ok(())
        }
    }
}

async fn handle_interrupt(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
) -> Result<bool, LoopEngineError> {
    if cancel.is_cancelled() || run.status() == RunStatus::Cancelling {
        cancel_run(run, execution, port).await?;
        return Ok(true);
    }
    // #1272: if the run is already terminal (e.g. Failed after a
    // timeout inside execute_step), return immediately without
    // attempting another timeout transition.
    if run.status().is_terminal() {
        return Ok(true);
    }
    if run.has_timed_out(Instant::now()) {
        timeout_run(run, execution, port).await?;
        return Ok(true);
    }
    Ok(false)
}

async fn timeout_run(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
) -> Result<(), LoopEngineError> {
    fail_run(
        run,
        execution,
        port,
        format!(
            "run timed out after {} seconds",
            run.spec().timeout.as_secs()
        ),
    )
    .await
}

pub(crate) async fn fail_run(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    error: String,
) -> Result<(), LoopEngineError> {
    if !port.claim_terminal(run.id()) {
        return cancel_run(run, execution, port).await;
    }
    run.fail(error)?;
    emit_events(run, execution, port).await
}

async fn cancel_run(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
) -> Result<(), LoopEngineError> {
    let active_step = run.active_step_id();
    if run.status() != RunStatus::Cancelling {
        if !port.claim_cancellation(run.id()) {
            log::debug!(
                target: crate::LOG_TARGET,
                "[cancel_run] cancellation not claimed (owned by another port) run_id={}",
                short(run.id()),
            );
            return Ok(());
        }
        match run.request_cancellation() {
            RunCancellationRequest::Accepted | RunCancellationRequest::AlreadyCancelling => {}
            RunCancellationRequest::AlreadyTerminal => return Ok(()),
        }
        log::debug!(
            target: crate::LOG_TARGET,
            "[cancel_run] phase1 CancellationRequested run_id={}",
            short(run.id()),
        );
        emit_events(run, execution, port).await?;
    }
    log::debug!(
        target: crate::LOG_TARGET,
        "[cancel_run] phase2 finish_cancellation run_id={}",
        short(run.id()),
    );
    if let Some(step_id) = &active_step {
        run_step_finalization_phase(
            execution,
            port.persistence_mut(),
            step_id,
            crate::ports::FinalizeCause::UserCancelledStep,
        )
        .await?;
    }
    run.finish_cancellation()?;
    emit_events(run, execution, port).await
}

async fn emit_events(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
) -> Result<(), LoopEngineError> {
    let events = run.drain_events();
    if events.is_empty() {
        return Ok(());
    }
    if let Err(error) = port.emit(execution, events.clone()).await {
        run.restore_events(events);
        return Err(error);
    }
    Ok(())
}

fn short(id: &sdk::RunId) -> String {
    let s = id.to_string();
    if s.len() > 8 {
        s.split_at(8).0.to_string()
    } else {
        s
    }
}

fn model_step_label(step: &ModelStep) -> &'static str {
    match step {
        ModelStep::Complete { .. } => "Complete",
        #[cfg(test)]
        ModelStep::Continue { .. } => "Continue",
        ModelStep::Tools { .. } => "Tools",
    }
}

use super::*;

/// Monotonic per-Run drain epoch. Each successful drain call increments
/// the epoch. Callers pass their expected epoch for mismatch detection
/// (#1272 per-run drain-or-seal linearization).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DrainEpoch(pub u64);

impl DrainEpoch {
    /// Advance to the next epoch.
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptedUserInput {
    UserMessage {
        input_id: sdk::InputId,
        text: String,
        images: Vec<sdk::ChatInputImage>,
    },
    SkillRequest(sdk::SkillRequest),
}

impl AcceptedUserInput {
    pub fn from_event(event: sdk::ChatInputEvent) -> Result<Self, sdk::ChatInputEvent> {
        match event {
            sdk::ChatInputEvent::UserMessage { id, text, images } => Ok(Self::UserMessage {
                input_id: id,
                text,
                images,
            }),
            sdk::ChatInputEvent::SkillRequest(request) => Ok(Self::SkillRequest(request)),
            other => Err(other),
        }
    }

    pub fn into_event(self) -> sdk::ChatInputEvent {
        match self {
            Self::UserMessage {
                input_id,
                text,
                images,
            } => sdk::ChatInputEvent::UserMessage {
                id: input_id,
                text,
                images,
            },
            Self::SkillRequest(request) => sdk::ChatInputEvent::SkillRequest(request),
        }
    }

    pub fn input_id(&self) -> &sdk::InputId {
        match self {
            Self::UserMessage { input_id, .. } => input_id,
            Self::SkillRequest(request) => &request.input_id,
        }
    }

    pub fn model_message(&self) -> share::message::Message {
        match self {
            Self::UserMessage { text, images, .. } if images.is_empty() => {
                share::message::Message::user(text.clone())
            }
            Self::UserMessage { text, images, .. } => share::message::Message::user_with_images(
                text.clone(),
                images
                    .iter()
                    .cloned()
                    .map(|image| (image.id, image.base64, image.media_type))
                    .collect(),
            ),
            Self::SkillRequest(request) => share::message::Message::skill_request(
                crate::application::loop_engine::input::format_skill_request(request, "en"),
                share::message::SkillRequestMetadata {
                    skill: request.skill.clone(),
                    arguments: request.arguments.clone(),
                    raw_input: request.raw_input.clone(),
                },
            ),
        }
    }

    pub fn withdraw_text(&self) -> String {
        match self {
            Self::UserMessage { text, .. } => text.clone(),
            Self::SkillRequest(request) => request.raw_input.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopInput {
    pub text: String,
    pub input_id: Option<sdk::InputId>,
    pub images: Vec<sdk::ChatInputImage>,
    pub accepted: Option<AcceptedUserInput>,
}

impl LoopInput {
    pub fn accepted(input: AcceptedUserInput) -> Self {
        let input_id = Some(input.input_id().clone());
        let message = input.model_message();
        Self {
            text: message.text_content(),
            input_id,
            images: Vec::new(),
            accepted: Some(input),
        }
    }

    pub fn input_id(&self) -> Option<&sdk::InputId> {
        self.input_id.as_ref()
    }

    pub fn message(&self) -> share::message::Message {
        if let Some(input) = self.accepted.as_ref() {
            return input.model_message();
        }
        if self.images.is_empty() {
            share::message::Message::user(self.text.clone())
        } else {
            share::message::Message::user_with_images(
                self.text.clone(),
                self.images
                    .iter()
                    .cloned()
                    .map(|image| (image.id, image.base64, image.media_type))
                    .collect(),
            )
        }
    }
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
/// #1272 Per-run drain-or-seal contract:
/// - `Ready` carries a **non-empty** batch of user input.
/// - `InternalContinuation` is for engine-driven continuations: stop hook
///   feedback or recorded tool results. The batch can be empty (pure
///   continuation) or carry any user input that arrived alongside it.
/// - `EmptyAndSealed` is the unique terminal gate.
///
/// Each variant carries a [`DrainEpoch`] for per-run linearization.
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
    /// per-run linearization (#1272).
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
    #[error("activity coordination error: {0}")]
    Activity(#[from] ActivityError),
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
    fn register_step_scope(
        &self,
        run_id: &sdk::RunId,
        step_id: sdk::RunStepId,
        cancel: CancellationToken,
    );
    fn clear_step_scope(&self, run_id: &sdk::RunId, step_id: &sdk::RunStepId);
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

pub(super) fn loop_input_message(input: &LoopInput) -> share::message::Message {
    input.message()
}

pub(super) fn freeze_step<P>(
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
                    .input_id()
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
    pub duration_ms: Option<u64>,
    pub messages: Vec<share::message::Message>,
    pub receipts: Vec<crate::ports::StepReceipt>,
}

pub(super) fn prepare_step_commit(
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
        duration_ms: execution
            .step_elapsed()
            .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX)),
        messages: execution.step_outcome(),
        receipts: Vec::new(),
    }
}

pub(super) async fn finalize_step<P>(
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
    async fn load_step_receipts(
        &mut self,
        _request: &crate::ports::ContextRequest,
    ) -> Result<Vec<crate::ports::StepReceipt>, LoopEngineError> {
        Ok(Vec::new())
    }
    async fn persist_step_commit(&mut self, _commit: &StepCommit) -> Result<(), LoopEngineError> {
        Ok(())
    }
}

/// Compact 进度视图回调（#1500）：`(stage, current, total)`——
/// `current/total` 为 map-reduce chunk 计数，单次摘要时为 `None`。
/// Loop Engine 提供实现（转发到 Activity 观测），Runtime 透传给 Context。
/// 闭包形式可自动实现（F: Fn + Send + Sync）。
pub trait CompactProgressView: Send + Sync {
    fn emit(&self, stage: sdk::CompactStageView, current: Option<u32>, total: Option<u32>);
}

impl<F> CompactProgressView for F
where
    F: Fn(sdk::CompactStageView, Option<u32>, Option<u32>) + Send + Sync,
{
    fn emit(&self, stage: sdk::CompactStageView, current: Option<u32>, total: Option<u32>) {
        self(stage, current, total)
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
        progress: std::sync::Arc<dyn CompactProgressView>,
    ) -> Result<(), LoopEngineError>;
}

#[async_trait]
pub trait ModelInvocationPort: Send {
    async fn invoke_model(
        &mut self,
        execution: &mut RunExecutionState,
        step_id: &sdk::RunStepId,
        cancel: &CancellationToken,
    ) -> Result<(ModelStep, StepTokenUsage), LoopEngineError>;

    /// #1494：取走边流边执行的旁路结果（流中 ToolCallCompleted 已执行完的轮次）。
    /// 非空时 engine 的 Tools 阶段跳过 `execute_tools`，直接汇总缓冲结果。
    async fn take_streaming_tool_results(
        &mut self,
    ) -> Vec<crate::application::loop_engine::chat::streaming_tool::StreamingToolRoundResult> {
        Vec::new()
    }
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

    /// #1494：汇总边流边执行的旁路结果（不再执行工具，只 materialize + 状态登记）。
    async fn finalize_streaming_tool_results(
        &mut self,
        execution: &mut RunExecutionState,
        step_id: &sdk::RunStepId,
        rounds: Vec<
            crate::application::loop_engine::chat::streaming_tool::StreamingToolRoundResult,
        >,
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

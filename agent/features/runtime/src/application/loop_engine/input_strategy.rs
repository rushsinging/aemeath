//! Input-strategy trait and concrete implementations for Main and Sub adapters.
//!
//! The [`InputStrategy`] trait abstracts the input source: the Main adapter
//! feeds from a channel + run-scoped buffer, while the Sub adapter feeds from
//! a fixed prompt with epoch tracking.
//!
//! #1272 Per-turn drain-or-seal is the contract both strategies must honour.

use sdk::ChatInputEvent;
use share::message::Message;

use crate::application::loop_engine::chat::run_input_buffer::BufferDrain;
use crate::application::loop_engine::chat::{
    ChatEventSink, ChatEventSinkHandle, InputEventDrainPort, PendingInputBuffer, QueueDrainPort,
    RuntimeStreamEvent,
};
use crate::application::loop_engine::{
    DrainEpoch, DrainOutcome, InternalContinuationKind, LoopEngineError,
};

#[derive(Clone, Default)]
pub(crate) struct InputContinuationState {
    stop_hook_feedback: std::sync::Arc<std::sync::Mutex<Option<Message>>>,
    pending_step_prefix: std::sync::Arc<std::sync::Mutex<Option<Message>>>,
    tool_results_pending: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl InputContinuationState {
    pub(crate) fn install_stop_hook_feedback(&self, message: Message) {
        *self
            .stop_hook_feedback
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(message);
    }

    fn take_stop_hook_feedback(&self) -> Option<Message> {
        self.stop_hook_feedback
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }

    fn set_pending_step_prefix(&self, message: Message) {
        *self
            .pending_step_prefix
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(message);
    }

    pub(crate) fn schedule_tool_results(&self) {
        self.tool_results_pending
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn take_tool_results(&self) -> bool {
        self.tool_results_pending
            .swap(false, std::sync::atomic::Ordering::AcqRel)
    }

    pub(crate) fn take_step_prefix(&self) -> Option<Message> {
        self.pending_step_prefix
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }
}

/// Common interface for input-source strategies.
///
/// Each adapter holds a concrete strategy and delegates [`drain_input`] and
/// [`await_user_input`] through it.  Because the two strategies have
/// fundamentally different state (channel-based vs fixed-prompt), the trait
/// exists for interface consistency, not for dynamic dispatch.
#[async_trait::async_trait]
pub(crate) trait InputStrategy {
    /// Drain the next batch of input.  Called by the engine when the Run is
    /// not awaiting user input.
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError>;

    /// Drain input while the Run is `AwaitingUser`.  Must never seal the
    /// input buffer on empty — the buffer stays receptive to future user
    /// input within the same Run (#1272).
    async fn await_user_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError>;
}

// ── Main adapter strategy ──────────────────────────────────────────────

/// Input strategy for the **Main** adapter.
///
/// #1385 Task 12: `sink` is now a [`ChatEventSinkHandle`] (shared with
/// [`RuntimeContext`]) instead of a generic `&S`.  This eliminates the `S`
/// generic parameter.
#[derive(Clone)]
pub(crate) struct BufferedInputAdapter<Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    pub input_events: I,
    /// #1385 Task 12: Canonical event sink from RuntimeContext, not a separate
    /// sink reference.  This is Clone and implements ChatEventSink directly.
    pub sink: ChatEventSinkHandle,
    pub queue: Q,
    /// Non-user-message events (controls) are forwarded here for the
    /// session idle gate to process after the Run ends.
    pub pending_input: PendingInputBuffer,
    /// #1385 Task 12: Run-scoped input buffer handle shared with RuntimeContext.
    /// User messages received during this Run are accumulated here and drained
    /// per-step within the same Run (#1272).  All access goes through
    /// [`RunInputBufferHandle::with_lock`].
    pub run_input_buffer: crate::application::run::context::RunInputBufferHandle,
    /// Stop-hook feedback, step-prefix relay and tool-results continuation.
    pub continuation: InputContinuationState,
    pub run_id: sdk::RunId,
}

impl<Q, I> BufferedInputAdapter<Q, I>
where
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    pub(crate) fn drain_remaining_events(&mut self) {
        let sealed = self.run_input_buffer.is_sealed();
        let drained = self.run_input_buffer.with_lock(|buffer| buffer.drain_all());
        for event in drained {
            if matches!(event, ChatInputEvent::UserMessage { .. }) && sealed {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "BufferedInputAdapter: sealed buffer contained unconsumed UserMessage; routing to pending input"
                );
            }
            self.pending_input.push(event);
        }
    }

    /// Unify UserMessage admission into the active Run's input buffer.
    /// Uses `push_or_reject`: when the buffer is sealed, the message is
    /// routed to `pending_input` for the next Run; when accepted,
    /// `UserMessagesQueued` is emitted.
    pub async fn admit_user_message(&mut self, event: ChatInputEvent) {
        debug_assert!(matches!(event, ChatInputEvent::UserMessage { .. }));
        let (rejected, queued) = self.run_input_buffer.with_lock(|buf| {
            let rejected = buf.push_or_reject(event);
            let queued = buf.user_message_snapshot();
            (rejected, queued)
        });
        match rejected {
            Some(rejected) => {
                let rejected_id = match &rejected {
                    ChatInputEvent::UserMessage { id, .. } => Some(id.as_str().to_string()),
                    _ => None,
                };
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[loop_debug] admit_user_message run_id={} REJECTED sealed=true rejected_id={:?}",
                    self.run_id,
                    rejected_id,
                );
                self.pending_input.push(rejected);
            }
            None => {
                let queued_ids: Vec<_> = queued
                    .iter()
                    .map(|(id, _)| id.as_str().to_string())
                    .collect();
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[loop_debug] admit_user_message run_id={} ACCEPTED queue_count={} queued_ids={:?}",
                    self.run_id,
                    queued.len(),
                    queued_ids,
                );
                self.sink
                    .send_event(RuntimeStreamEvent::UserMessagesQueued { queued })
                    .await;
            }
        }
    }

    /// Collect events from channel sources and check for internal
    /// continuations (stop-hook feedback or tool results).  Returns
    /// `Some(outcome)` if a continuation is ready, `None` if control
    /// falls through to the normal drain path.
    async fn drain_collect_continuations(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<Option<DrainOutcome>, LoopEngineError> {
        let mut events = self.input_events.drain_input_events().await;
        if let Some(queued) = self.queue.drain_queued_input().await {
            events.extend(
                queued
                    .into_iter()
                    .map(|text| ChatInputEvent::classify_text(text, Vec::new())),
            );
        }
        for event in events {
            match event {
                ChatInputEvent::UserMessage { .. } => self.admit_user_message(event).await,
                ChatInputEvent::WithdrawAll => {
                    let texts = self
                        .run_input_buffer
                        .with_lock(|b| b.withdraw_all_user_texts());
                    if !texts.is_empty() {
                        self.sink
                            .send_event(RuntimeStreamEvent::UserMessagesWithdrawn { texts })
                            .await;
                    }
                }
                other => self.pending_input.push(other),
            }
        }

        // #1272 Per-turn drain-or-seal contract:
        //   StopHookFeedback > ToolResults > user input (Ready) > EmptyAndSealed.
        if let Some(feedback) = self.continuation.take_stop_hook_feedback() {
            let text = feedback.text_content();
            self.continuation.set_pending_step_prefix(feedback);
            let (batch, epoch) = match self
                .run_input_buffer
                .with_lock(|b| b.take_internal_continuation(expected_epoch))
            {
                BufferDrain::Ready { batch, epoch } => (batch, epoch),
                BufferDrain::EmptyAndSealed { .. } | BufferDrain::Empty { .. } => {
                    return Err(LoopEngineError::Adapter(
                        "internal continuation 意外返回 EmptyAndSealed/Empty".to_string(),
                    ));
                }
                BufferDrain::AlreadySealed { epoch } => {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "BufferedInputAdapter: take_internal_continuation returned AlreadySealed at epoch {:?}",
                        epoch,
                    );
                    return Ok(Some(DrainOutcome::EmptyAndSealed { epoch }));
                }
                BufferDrain::EpochMismatch { expected, actual } => {
                    return Err(LoopEngineError::Adapter(format!(
                        "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                        expected, actual,
                    )));
                }
            };
            let input_ids: Vec<_> = batch
                .iter()
                .filter_map(|i| i.input_id.as_ref().map(|id| id.as_str().to_string()))
                .collect();
            log::debug!(
                target: crate::LOG_TARGET,
                "[loop_debug] drain_input run_id={} status=InternalContinuation epoch={:?} kind=StopHookFeedback input_ids={:?} count={}",
                self.run_id,
                epoch,
                input_ids,
                batch.len(),
            );
            return Ok(Some(DrainOutcome::InternalContinuation {
                kind: InternalContinuationKind::StopHookFeedback { feedback: text },
                batch,
                epoch,
            }));
        }
        if self.continuation.take_tool_results() {
            let (batch, epoch) = match self
                .run_input_buffer
                .with_lock(|b| b.take_internal_continuation(expected_epoch))
            {
                BufferDrain::Ready { batch, epoch } => (batch, epoch),
                BufferDrain::EmptyAndSealed { .. } | BufferDrain::Empty { .. } => {
                    return Err(LoopEngineError::Adapter(
                        "internal continuation 意外返回 EmptyAndSealed/Empty".to_string(),
                    ));
                }
                BufferDrain::AlreadySealed { epoch } => {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "BufferedInputAdapter: take_internal_continuation returned AlreadySealed at epoch {:?}",
                        epoch,
                    );
                    return Ok(Some(DrainOutcome::EmptyAndSealed { epoch }));
                }
                BufferDrain::EpochMismatch { expected, actual } => {
                    return Err(LoopEngineError::Adapter(format!(
                        "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                        expected, actual,
                    )));
                }
            };
            let input_ids: Vec<_> = batch
                .iter()
                .filter_map(|i| i.input_id.as_ref().map(|id| id.as_str().to_string()))
                .collect();
            log::debug!(
                target: crate::LOG_TARGET,
                "[loop_debug] drain_input run_id={} status=InternalContinuation epoch={:?} kind=ToolResults input_ids={:?} count={}",
                self.run_id,
                epoch,
                input_ids,
                batch.len(),
            );
            return Ok(Some(DrainOutcome::InternalContinuation {
                kind: InternalContinuationKind::ToolResults,
                batch,
                epoch,
            }));
        }

        // Fall through to normal drain path
        Ok(None)
    }
}

#[async_trait::async_trait]
impl<Q, I> InputStrategy for BufferedInputAdapter<Q, I>
where
    Q: QueueDrainPort + Send,
    I: InputEventDrainPort + Send,
{
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        if let Some(outcome) = self.drain_collect_continuations(expected_epoch).await? {
            return Ok(outcome);
        }

        // #1272: atomic drain-or-seal — a single synchronous decision point
        // instead of drain-then-check. Once sealed, late UserMessages are
        // rejected by push_or_reject (not silently buffered for next Run).
        match self
            .run_input_buffer
            .with_lock(|b| b.drain_or_seal(expected_epoch))
        {
            BufferDrain::Ready { batch, epoch } => {
                let input_ids: Vec<_> = batch
                    .iter()
                    .filter_map(|i| i.input_id.as_ref().map(|id| id.as_str().to_string()))
                    .collect();
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[loop_debug] drain_input run_id={} status=Ready epoch={:?} kind=per_turn input_ids={:?} count={}",
                    self.run_id,
                    epoch,
                    input_ids,
                    batch.len(),
                );
                Ok(DrainOutcome::Ready { batch, epoch })
            }
            BufferDrain::EmptyAndSealed { epoch } => {
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[loop_debug] drain_input run_id={} status=EmptyAndSealed epoch={:?}",
                    self.run_id,
                    epoch,
                );
                Ok(DrainOutcome::EmptyAndSealed { epoch })
            }
            BufferDrain::Empty { .. } => Err(LoopEngineError::Adapter(
                "drain_or_seal 意外返回 Empty".to_string(),
            )),
            BufferDrain::AlreadySealed { epoch } => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "BufferedInputAdapter: drain_or_seal returned AlreadySealed — buffer was already sealed"
                );
                Ok(DrainOutcome::EmptyAndSealed { epoch })
            }
            BufferDrain::EpochMismatch { expected, actual } => {
                log::error!(
                    target: crate::LOG_TARGET,
                    "BufferedInputAdapter: drain_or_seal epoch mismatch — expected {:?}, actual {:?}",
                    expected,
                    actual,
                );
                Err(LoopEngineError::Adapter(format!(
                    "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                    expected, actual,
                )))
            }
        }
    }

    /// #1280: AwaitUser 时直接 async 等 input_events channel。
    /// 收到 UserMessage → push RunInputBuffer → drain 返回 Ready。
    /// 收到非 UserMessage → push pending_input → 继续等。
    /// channel 关闭 → EmptyAndSealed。
    /// cancel/timeout 由 engine 的 await_interruptible 自动处理（future drop）。
    async fn await_user_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        // First check if continuations or already-buffered input is ready.
        if let Some(outcome) = self.drain_collect_continuations(expected_epoch).await? {
            return Ok(outcome);
        }

        // Check RunInputBuffer (might have been seeded during drain phase).
        if let Some(outcome) = match self
            .run_input_buffer
            .with_lock(|b| b.try_drain_unsealed(expected_epoch))
        {
            BufferDrain::Ready { batch, epoch } => Some(DrainOutcome::Ready { batch, epoch }),
            BufferDrain::Empty { .. } | BufferDrain::EmptyAndSealed { .. } => None,
            BufferDrain::AlreadySealed { epoch } => {
                return Ok(DrainOutcome::EmptyAndSealed { epoch });
            }
            BufferDrain::EpochMismatch { expected, actual } => {
                return Err(LoopEngineError::Adapter(format!(
                    "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                    expected, actual,
                )));
            }
        } {
            return Ok(outcome);
        }

        // Async park: wait for the next input event from the channel.
        // engine's await_interruptible wraps this future — cancel/timeout
        // will drop it automatically.
        let event = self.input_events.recv_next_input().await;
        match event {
            None => {
                // Channel closed — seal.
                Ok(DrainOutcome::EmptyAndSealed {
                    epoch: expected_epoch,
                })
            }
            Some(event @ ChatInputEvent::UserMessage { .. }) => {
                let outcome = self.run_input_buffer.with_lock(|b| {
                    b.push(event);
                    b.try_drain_unsealed(expected_epoch)
                });
                match outcome {
                    BufferDrain::Ready { batch, epoch } => Ok(DrainOutcome::Ready { batch, epoch }),
                    BufferDrain::Empty { epoch } => Ok(DrainOutcome::NoInput { epoch }),
                    BufferDrain::EmptyAndSealed { epoch }
                    | BufferDrain::AlreadySealed { epoch } => {
                        Ok(DrainOutcome::EmptyAndSealed { epoch })
                    }
                    BufferDrain::EpochMismatch { expected, actual } => {
                        Err(LoopEngineError::Adapter(format!(
                            "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                            expected, actual,
                        )))
                    }
                }
            }
            Some(other) => {
                // Non-UserMessage command: defer to session idle gate.
                self.pending_input.push(other);
                Ok(DrainOutcome::EmptyAndSealed {
                    epoch: expected_epoch,
                })
            }
        }
    }
}

#[async_trait::async_trait]
impl<Q, I> crate::application::loop_engine::InputPort for BufferedInputAdapter<Q, I>
where
    Q: QueueDrainPort + Send,
    I: InputEventDrainPort + Send,
{
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        InputStrategy::drain_input(self, expected_epoch).await
    }

    fn schedule_internal_continuation(&mut self, kind: InternalContinuationKind) {
        if matches!(kind, InternalContinuationKind::ToolResults) {
            self.continuation.schedule_tool_results();
        }
    }

    async fn await_user_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        InputStrategy::await_user_input(self, expected_epoch).await
    }
}

// ── Sub adapter strategy ───────────────────────────────────────────────

/// Input strategy for the **Sub** adapter.
///
/// The Sub adapter has a fixed prompt that is drained as `Ready` exactly
/// once (epoch 0), then `InternalContinuation::ToolResults` for each
/// subsequent tool-result turn, and finally `EmptyAndSealed` when the model
/// produces no further tool calls.
pub(crate) struct FixedInputAdapter<'a> {
    pub prompt: &'a str,
    /// Whether the initial prompt has already been consumed (#1272).
    pub prompt_drained: bool,
    /// Sub maintains its own epoch counter for per-turn drain linearization.
    /// First drain (Ready) uses epoch 0, then advances to 1; subsequent
    /// continuations/seal use the current epoch.
    pub next_epoch: DrainEpoch,
    /// Tracks whether the last step executed tools. When true, drain_input
    /// returns `InternalContinuation::ToolResults` so the engine invokes the
    /// model again with tool results (instead of prematurely sealing).
    pub has_tool_results_pending: bool,
}

impl<'a> FixedInputAdapter<'a> {
    pub fn new(prompt: &'a str) -> Self {
        Self {
            prompt,
            prompt_drained: false,
            next_epoch: DrainEpoch(0),
            has_tool_results_pending: false,
        }
    }
}

#[async_trait::async_trait]
impl InputStrategy for FixedInputAdapter<'_> {
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        // #1272: Sub's fixed-prompt strategy returns the prompt as Ready
        // exactly once (consumed by the first step's accepted-input handoff)
        // at epoch 0, then EmptyAndSealed at epoch 1 forever after.
        if !self.prompt_drained {
            if expected_epoch != self.next_epoch {
                return Err(LoopEngineError::Adapter(format!(
                    "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                    expected_epoch, self.next_epoch,
                )));
            }
            self.prompt_drained = true;
            let epoch = self.next_epoch;
            self.next_epoch = epoch.next();
            return Ok(DrainOutcome::Ready {
                batch: vec![crate::application::loop_engine::LoopInput {
                    text: self.prompt.to_string(),
                    input_id: None,
                    images: Vec::new(),
                }],
                epoch,
            });
        }
        if expected_epoch != self.next_epoch {
            return Err(LoopEngineError::Adapter(format!(
                "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                expected_epoch, self.next_epoch,
            )));
        }
        let epoch = self.next_epoch;
        self.next_epoch = epoch.next();
        // #1384: If the last step executed tools, return InternalContinuation
        // so the engine invokes the model again with tool results appended
        // to messages. Only seal when the model produced no tool calls
        // (ModelStep::Complete/Continue) — that's the terminal response.
        if self.has_tool_results_pending {
            self.has_tool_results_pending = false;
            return Ok(DrainOutcome::InternalContinuation {
                kind: InternalContinuationKind::ToolResults,
                batch: Vec::new(),
                epoch,
            });
        }
        Ok(DrainOutcome::EmptyAndSealed { epoch })
    }

    /// #1280: Sub Agent 的 await_user_input 预留接口。
    ///
    /// 当前 Sub 使用 FixedInputBuffer，drain 后立即 seal，永不进入 AwaitingUser，
    /// 因此此方法不可达。
    ///
    /// #1248 将注入 InteractionBridge 后激活：Sub 的 AskUserQuestion suspension
    /// 会触发 AwaitingUser，此方法 async park 等 InteractionBridge oneshot。
    async fn await_user_input(
        &mut self,
        _expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        Err(LoopEngineError::Adapter(
            "Sub Agent 不支持 AwaitingUser（FixedInputBuffer 只 drain 一次即 seal）\
             ; #1248 注入 InteractionBridge 后激活"
                .to_string(),
        ))
    }
}

#[async_trait::async_trait]
impl crate::application::loop_engine::InputPort for FixedInputAdapter<'_> {
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        InputStrategy::drain_input(self, expected_epoch).await
    }

    fn schedule_internal_continuation(&mut self, kind: InternalContinuationKind) {
        if matches!(kind, InternalContinuationKind::ToolResults) {
            self.has_tool_results_pending = true;
        }
    }

    async fn await_user_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        InputStrategy::await_user_input(self, expected_epoch).await
    }
}

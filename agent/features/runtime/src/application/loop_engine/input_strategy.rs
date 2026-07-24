//! Input-strategy trait and concrete implementations for Main and Sub adapters.
//!
//! The [`InputStrategy`] trait abstracts the input source: the Main adapter
//! feeds from a channel + run-scoped buffer, while the Sub adapter feeds from
//! a fixed prompt with epoch tracking.
//!
//! #1272 Per-turn drain-or-seal is the contract both strategies must honour.

use sdk::ChatInputEvent;
use share::message::Message;

use crate::application::loop_engine::{
    DrainEpoch, DrainOutcome, InternalContinuationKind, LoopEngineError,
};
use crate::application::main_loop::looping::run_input_buffer::{BufferDrain, RunInputBuffer};
use crate::application::main_loop::looping::{
    ChatEventSink, InputEventDrainPort, PendingInputBuffer, QueueDrainPort, RuntimeStreamEvent,
};

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
/// Owns the run-scoped [`RunInputBuffer`] and the continuation flags shared
/// between drain and freeze/execute-tool phases.  Holds references to the
/// channel-based input sources (`input_events`, `queue`), event sink, and
/// pending-input buffer.
pub(crate) struct MainInputStrategy<'a, S, Q, I>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    pub input_events: &'a I,
    pub sink: &'a S,
    pub queue: &'a Q,
    /// Non-user-message events (controls) are forwarded here for the
    /// session idle gate to process after the Run ends.
    pub pending_input: &'a mut PendingInputBuffer,
    /// Run-scoped input buffer: user messages received during this Run are
    /// accumulated here and drained per-step within the same Run (#1272).
    pub run_input_buffer: RunInputBuffer,
    /// Stop-hook feedback set by `invoke_model`, consumed by drain to
    /// produce `InternalContinuation::StopHookFeedback`.
    pub stop_hook_feedback: Option<Message>,
    /// Bridge between drain and freeze: drain moves the feedback here,
    /// and `freeze_step` consumes it for injection.
    pub pending_stop_hook_feedback: Option<Message>,
    /// Set by `execute_tools`; cleared by drain to produce
    /// `InternalContinuation::ToolResults`.
    pub pending_tool_results: bool,
    pub run_id: sdk::RunId,
}

impl<'a, S, Q, I> MainInputStrategy<'a, S, Q, I>
where
    S: ChatEventSink,
    Q: QueueDrainPort,
    I: InputEventDrainPort,
{
    /// Unify UserMessage admission into the active Run's input buffer.
    /// Uses `push_or_reject`: when the buffer is sealed, the message is
    /// routed to `pending_input` for the next Run; when accepted,
    /// `UserMessagesQueued` is emitted.
    pub async fn admit_user_message(&mut self, event: ChatInputEvent) {
        debug_assert!(matches!(event, ChatInputEvent::UserMessage { .. }));
        match self.run_input_buffer.push_or_reject(event) {
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
                let queued = self.run_input_buffer.user_message_snapshot();
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
                    let texts = self.run_input_buffer.withdraw_all_user_texts();
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
        if let Some(feedback) = self.stop_hook_feedback.take() {
            let text = feedback.text_content();
            self.pending_stop_hook_feedback = Some(feedback);
            let (batch, epoch) = match self
                .run_input_buffer
                .take_internal_continuation(expected_epoch)
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
                        "MainInputStrategy: take_internal_continuation returned AlreadySealed at epoch {:?}",
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
        if self.pending_tool_results {
            self.pending_tool_results = false;
            let (batch, epoch) = match self
                .run_input_buffer
                .take_internal_continuation(expected_epoch)
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
                        "MainInputStrategy: take_internal_continuation returned AlreadySealed at epoch {:?}",
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
impl<S, Q, I> InputStrategy for MainInputStrategy<'_, S, Q, I>
where
    S: ChatEventSink + Send,
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
        match self.run_input_buffer.drain_or_seal(expected_epoch) {
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
                    "MainInputStrategy: drain_or_seal returned AlreadySealed — buffer was already sealed"
                );
                Ok(DrainOutcome::EmptyAndSealed { epoch })
            }
            BufferDrain::EpochMismatch { expected, actual } => {
                log::error!(
                    target: crate::LOG_TARGET,
                    "MainInputStrategy: drain_or_seal epoch mismatch — expected {:?}, actual {:?}",
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
        if let Some(outcome) = match self.run_input_buffer.try_drain_unsealed(expected_epoch) {
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
        loop {
            let event = self.input_events.recv_next_input().await;
            match event {
                None => {
                    // Channel closed — seal.
                    return Ok(DrainOutcome::EmptyAndSealed {
                        epoch: expected_epoch,
                    });
                }
                Some(ChatInputEvent::UserMessage { .. }) => {
                    self.run_input_buffer.push(event.unwrap());
                    return match self.run_input_buffer.try_drain_unsealed(expected_epoch) {
                        BufferDrain::Ready { batch, epoch } => {
                            Ok(DrainOutcome::Ready { batch, epoch })
                        }
                        BufferDrain::Empty { epoch } => Ok(DrainOutcome::NoInput { epoch }),
                        BufferDrain::EmptyAndSealed { epoch } => {
                            Ok(DrainOutcome::EmptyAndSealed { epoch })
                        }
                        BufferDrain::AlreadySealed { epoch } => {
                            Ok(DrainOutcome::EmptyAndSealed { epoch })
                        }
                        BufferDrain::EpochMismatch { expected, actual } => {
                            Err(LoopEngineError::Adapter(format!(
                                "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                                expected, actual,
                            )))
                        }
                    };
                }
                Some(other) => {
                    // Non-UserMessage command: defer to session idle gate.
                    // Return EmptyAndSealed so the Run completes and the
                    // session gate can process the command.
                    self.pending_input.push(other);
                    return Ok(DrainOutcome::EmptyAndSealed {
                        epoch: expected_epoch,
                    });
                }
            }
        }
    }
}

// ── Sub adapter strategy ───────────────────────────────────────────────

/// Input strategy for the **Sub** adapter.
///
/// The Sub adapter has a fixed prompt that is drained as `Ready` exactly
/// once (epoch 0), then `InternalContinuation::ToolResults` for each
/// subsequent tool-result turn, and finally `EmptyAndSealed` when the model
/// produces no further tool calls.
pub(crate) struct SubInputStrategy<'a> {
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

impl<'a> SubInputStrategy<'a> {
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
impl InputStrategy for SubInputStrategy<'_> {
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

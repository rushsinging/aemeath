//! Input-strategy trait and concrete implementations for Main and Sub adapters.
//!
//! The [`InputStrategy`] trait abstracts the input source: the Main adapter
//! feeds from a channel + run-scoped buffer, while the Sub adapter feeds from
//! a fixed prompt with epoch tracking.
//!
//! #1272 Per-run drain-or-seal is the contract both strategies must honour.

use sdk::ChatInputEvent;
use share::message::Message;

use crate::application::loop_engine::chat::run_input_buffer::BufferDrain;
use crate::application::loop_engine::chat::{
    ChatEventSink, ChatEventSinkHandle, InputEventDrainPort, PendingInputBuffer, RuntimeStreamEvent,
};
use crate::application::loop_engine::{
    DrainEpoch, DrainOutcome, InternalContinuationKind, LoopEngineError,
};

#[derive(Clone, Default)]
pub(crate) struct InputContinuationState {
    stop_hook_feedback: std::sync::Arc<std::sync::Mutex<Option<Message>>>,
    pending_step_prefix: std::sync::Arc<std::sync::Mutex<Option<Message>>>,
    tool_results_pending: std::sync::Arc<std::sync::atomic::AtomicBool>,
    background_process_wakeup: std::sync::Arc<std::sync::atomic::AtomicBool>,
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

    /// 预置后台进程 wakeup 续延（#252）：wakeup Run 空输入启动时装配，
    /// 首次 drain 以 InternalContinuation 驱动一次 step，使 reminder
    /// 管线（完成事实注入）真正到达 LLM。
    pub(crate) fn install_background_process_wakeup(&self) {
        self.background_process_wakeup
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn take_background_process_wakeup(&self) -> bool {
        self.background_process_wakeup
            .swap(false, std::sync::atomic::Ordering::AcqRel)
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

pub(crate) trait SessionInputPort: InputEventDrainPort {
    fn defer(&self, event: ChatInputEvent);
}

fn chat_input_event_kind(event: &ChatInputEvent) -> &'static str {
    match event {
        ChatInputEvent::UserMessage { .. } => "user_message",
        ChatInputEvent::SkillRequest(_) => "skill_request",
        ChatInputEvent::WithdrawAll => "withdraw_all",
        _ => "control",
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
/// #1385 TaskData 12: `sink` is now a [`ChatEventSinkHandle`] (shared with
/// [`RuntimeContext`]) instead of a generic `&S`.  This eliminates the `S`
/// generic parameter.
#[derive(Clone)]
pub(crate) struct BufferedInputAdapter<I>
where
    I: SessionInputPort,
{
    pub input_events: I,
    /// #1385 TaskData 12: Canonical event sink from RuntimeContext, not a separate
    /// sink reference.  This is Clone and implements ChatEventSink directly.
    pub sink: ChatEventSinkHandle,
    /// Non-user-message events (controls) are forwarded here for the
    /// session idle gate to process after the Run ends.
    pub pending_input: PendingInputBuffer,
    /// #1385 TaskData 12: Run-scoped input buffer handle shared with RuntimeContext.
    /// User messages received during this Run are accumulated here and drained
    /// per-step within the same Run (#1272).  All access goes through
    /// [`RunInputBufferHandle::with_lock`].
    pub run_input_buffer: crate::application::run::context::RunInputBufferHandle,
    /// Stop-hook feedback, step-prefix relay and tool-results continuation.
    pub continuation: InputContinuationState,
    pub run_id: sdk::RunId,
}

impl<I> BufferedInputAdapter<I>
where
    I: SessionInputPort,
{
    pub(crate) fn drain_remaining_events(&mut self) {
        let sealed = self.run_input_buffer.is_sealed();
        let drained = self.run_input_buffer.with_lock(|buffer| buffer.drain_all());
        for event in drained {
            if matches!(event, ChatInputEvent::UserMessage { .. }) && sealed {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "BufferedInputAdapter: sealed buffer contained unconsumed UserMessage; routing to Session mailbox"
                );
            }
            self.input_events.defer(event);
        }
        // 裁决 3：busy 期间积压的 `/reflect-now` NEVER 排队执行——Run 收尾回流
        // pending buffer 前统一丢弃并逐条提示（busy 积压的主拦截点）。
        crate::application::loop_engine::chat::input_gate::drop_queued_reflect_now(
            &self.pending_input,
            &self.sink,
        );
        for event in self.pending_input.drain_all() {
            self.input_events.defer(event);
        }
    }

    /// Unify UserMessage admission into the active Run's input buffer.
    /// Uses `push_or_reject`: when the buffer is sealed, the message is
    /// routed to `pending_input` for the next Run; when accepted,
    /// `UserMessagesQueued` is emitted.
    pub async fn admit_user_message(&mut self, event: ChatInputEvent) {
        debug_assert!(matches!(
            event,
            ChatInputEvent::UserMessage { .. } | ChatInputEvent::SkillRequest(_)
        ));
        let (rejected, queued) = self.run_input_buffer.with_lock(|buf| {
            let rejected = buf.push_or_reject(event);
            let queued = buf.user_message_snapshot();
            (rejected, queued)
        });
        match rejected {
            Some(rejected) => {
                let rejected_id = match &rejected {
                    ChatInputEvent::UserMessage { id, .. } => Some(id.as_str().to_string()),
                    ChatInputEvent::SkillRequest(request) => {
                        Some(request.input_id.as_str().to_string())
                    }
                    _ => None,
                };
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[loop_debug] admit_user_message run_id={} REJECTED sealed=true rejected_id={:?}",
                    self.run_id,
                    rejected_id,
                );
                self.input_events.defer(rejected);
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

    /// 发布控制类命令队列的全量快照（#1816）。
    ///
    /// 与 `apply_gate` 内的快照同源同形：入队是 gate 之外的三条路径之一，
    /// 必须在入队后立刻发布，否则 UI 会漏掉刚排队的命令。
    async fn publish_command_queue_snapshot(&self) {
        self.sink
            .send_event(RuntimeStreamEvent::ControlCommandsQueued {
                queued: self.pending_input.command_snapshot(),
            })
            .await;
    }

    /// Collect events from channel sources and check for internal
    /// continuations (stop-hook feedback or tool results).  Returns
    /// `Some(outcome)` if a continuation is ready, `None` if control
    /// falls through to the normal drain path.
    async fn drain_collect_continuations(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<Option<DrainOutcome>, LoopEngineError> {
        // #1818：一次 drain 就是一批，同批连续用户消息折叠为一条再接纳，
        // 这样 UserMessagesQueued 快照只含 1 条，TUI 排队行随之收敛为 1 组。
        let events = crate::application::loop_engine::batched_user_input::fold_batched_user_inputs(
            self.input_events.drain_input_events().await,
        );
        for event in events {
            match event {
                ChatInputEvent::UserMessage { .. } | ChatInputEvent::SkillRequest(_) => {
                    self.admit_user_message(event).await
                }
                ChatInputEvent::WithdrawAll => {
                    // 撤回语义覆盖所有待处理输入：Run 内消息 + 排队的控制命令（#1816）。
                    let mut texts = self
                        .run_input_buffer
                        .with_lock(|b| b.withdraw_all_user_texts());
                    let withdrawn_commands = self.pending_input.drain_for_withdraw();
                    if !withdrawn_commands.is_empty() {
                        // 命令队列已清空：发空快照让 UI 撤下命令行（#1816）。
                        texts.extend(withdrawn_commands);
                        self.publish_command_queue_snapshot().await;
                    }
                    if !texts.is_empty() {
                        self.sink
                            .send_event(RuntimeStreamEvent::UserMessagesWithdrawn { texts })
                            .await;
                    }
                }
                other => {
                    // 控制类命令入队：发布全量快照，UI 才知道命令已排队（#1816）。
                    self.pending_input.push(other);
                    self.publish_command_queue_snapshot().await;
                }
            }
        }

        // #1272 Per-run drain-or-seal contract:
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
                .filter_map(|input| input.input_id().map(|id| id.as_str().to_string()))
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
                .filter_map(|input| input.input_id().map(|id| id.as_str().to_string()))
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
        if self.continuation.take_background_process_wakeup() {
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
            log::debug!(
                target: crate::LOG_TARGET,
                "[loop_debug] drain_input run_id={} status=InternalContinuation epoch={:?} kind=BackgroundProcessWakeup count={}",
                self.run_id,
                epoch,
                batch.len(),
            );
            return Ok(Some(DrainOutcome::InternalContinuation {
                kind: InternalContinuationKind::BackgroundProcessWakeup,
                batch,
                epoch,
            }));
        }

        // Fall through to normal drain path
        Ok(None)
    }
}

#[async_trait::async_trait]
impl<I> InputStrategy for BufferedInputAdapter<I>
where
    I: SessionInputPort + Send,
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
                    .filter_map(|input| input.input_id().map(|id| id.as_str().to_string()))
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
        log::debug!(
            target: crate::LOG_TARGET,
            "[input_strategy] awaiting session input run_id={} epoch={:?}",
            self.run_id,
            expected_epoch,
        );
        let event = self.input_events.recv_next_input().await;
        log::debug!(
            target: crate::LOG_TARGET,
            "[input_strategy] session input wait completed run_id={} epoch={:?} event_kind={}",
            self.run_id,
            expected_epoch,
            event.as_ref().map(chat_input_event_kind).unwrap_or("source_closed"),
        );
        match event {
            None => {
                // Channel closed — seal.
                Ok(DrainOutcome::EmptyAndSealed {
                    epoch: expected_epoch,
                })
            }
            Some(
                event @ (ChatInputEvent::UserMessage { .. } | ChatInputEvent::SkillRequest(_)),
            ) => {
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
                self.publish_command_queue_snapshot().await;
                Ok(DrainOutcome::EmptyAndSealed {
                    epoch: expected_epoch,
                })
            }
        }
    }
}

#[async_trait::async_trait]
impl<I> crate::application::loop_engine::InputPort for BufferedInputAdapter<I>
where
    I: SessionInputPort + Send,
{
    /// 单用途 Run（Manual Compaction/Reflection）drain 出的用户输入回流 session
    /// 输入队列，由下一个会话 Run 消费；保留 accepted 元数据（原事件形态）。
    fn defer_user_batch(
        &mut self,
        batch: Vec<crate::application::loop_engine::engine::LoopInput>,
    ) -> Result<(), LoopEngineError> {
        for input in batch {
            let event = match input.accepted {
                Some(accepted) => accepted.into_event(),
                None => ChatInputEvent::UserMessage {
                    id: input.input_id.unwrap_or_else(sdk::InputId::new_v7),
                    text: input.text,
                    images: input.images,
                },
            };
            self.input_events.defer(event);
        }
        Ok(())
    }

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
/// subsequent tool-result run, and finally `EmptyAndSealed` when the model
/// produces no further tool calls.
pub(crate) struct FixedInputAdapter<'a> {
    pub prompt: &'a str,
    /// Whether the initial prompt has already been consumed (#1272).
    pub prompt_drained: bool,
    /// Sub maintains its own epoch counter for per-run drain linearization.
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
                    accepted: None,
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

#[cfg(test)]
#[path = "input_strategy_tests.rs"]
mod input_strategy_tests;

#[cfg(test)]
mod batched_user_input_tests {
    //! #1818：drain 路径同批折叠——快照必须只含 1 条，TUI 排队行才收敛为 1 组。

    use super::*;
    use crate::application::loop_engine::chat::{
        ChatEventSink, EventFuture, InputEventDrainPort, InputEventFuture, InputEventOptFuture,
        RuntimeStreamEvent,
    };
    use crate::application::loop_engine::InputPort;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct ScriptedInput {
        events: Arc<Mutex<Vec<ChatInputEvent>>>,
        deferred: Arc<Mutex<Vec<ChatInputEvent>>>,
    }

    impl ScriptedInput {
        fn with_events(events: Vec<ChatInputEvent>) -> Self {
            Self {
                events: Arc::new(Mutex::new(events)),
                deferred: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl InputEventDrainPort for ScriptedInput {
        fn drain_input_events<'a>(&'a self) -> InputEventFuture<'a> {
            Box::pin(async move { std::mem::take(&mut *self.events.lock().unwrap()) })
        }

        fn recv_next_input<'a>(&'a self) -> InputEventOptFuture<'a> {
            Box::pin(async move { self.events.lock().unwrap().pop() })
        }
    }

    impl SessionInputPort for ScriptedInput {
        fn defer(&self, event: ChatInputEvent) {
            self.deferred.lock().unwrap().push(event);
        }
    }

    #[derive(Clone, Default)]
    struct RecordingSink {
        events: Arc<Mutex<Vec<RuntimeStreamEvent>>>,
    }

    impl ChatEventSink for RecordingSink {
        fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
            Box::pin(async move {
                self.events.lock().unwrap().push(event);
            })
        }

        fn try_send_event(&self, event: RuntimeStreamEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    fn adapter_with(
        events: Vec<ChatInputEvent>,
    ) -> (BufferedInputAdapter<ScriptedInput>, RecordingSink) {
        let sink = RecordingSink::default();
        let adapter = BufferedInputAdapter {
            input_events: ScriptedInput::with_events(events),
            sink: ChatEventSinkHandle::new(sink.clone()),
            pending_input: PendingInputBuffer::default(),
            run_input_buffer: crate::application::run::context::RunInputBufferHandle::new(),
            continuation: InputContinuationState::default(),
            run_id: share::ids::RunId::new_v7(),
        };
        (adapter, sink)
    }

    /// 最后一次排队快照的 (InputId, 文本) 投影——消息类型细节与本测试无关。
    ///
    /// 取最后一条而非第一条：每接纳一条都会发一次快照，只有最后一次反映
    /// 整批接纳后的队列状态。
    fn queued_snapshot(sink: &RecordingSink) -> Option<Vec<(sdk::InputId, String)>> {
        sink.events
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|event| match event {
                RuntimeStreamEvent::UserMessagesQueued { queued } => Some(
                    queued
                        .iter()
                        .map(|(id, message)| (id.clone(), message.text_content().to_string()))
                        .collect(),
                ),
                _ => None,
            })
    }

    #[tokio::test]
    async fn drain_merges_same_batch_so_queued_snapshot_carries_one_message() {
        let (mut adapter, sink) = adapter_with(vec![
            ChatInputEvent::user_message("第一段", Vec::new()),
            ChatInputEvent::user_message("第二段", Vec::new()),
        ]);

        let outcome = adapter
            .drain_collect_continuations(DrainEpoch(0))
            .await
            .expect("drain 不应失败");

        assert!(outcome.is_none(), "没有内部 continuation 时返回 None");
        let snapshot = queued_snapshot(&sink).expect("接纳后必须发布排队快照");
        assert_eq!(
            snapshot.len(),
            1,
            "同批两条消息在快照里必须是 1 条，否则 TUI 会显示 2 组排队行"
        );
        assert_eq!(snapshot[0].1, "第一段\n\n第二段");
    }

    #[tokio::test]
    async fn drain_snapshot_keeps_skill_request_separate_from_merged_messages() {
        let (mut adapter, sink) = adapter_with(vec![
            ChatInputEvent::user_message("技能之前", Vec::new()),
            ChatInputEvent::SkillRequest(sdk::SkillRequest {
                input_id: sdk::InputId::new_v7(),
                skill: "superpowers:brainstorming".to_string(),
                arguments: "scope".to_string(),
                raw_input: "/superpowers:brainstorming scope".to_string(),
            }),
            ChatInputEvent::user_message("技能之后", Vec::new()),
        ]);

        adapter
            .drain_collect_continuations(DrainEpoch(0))
            .await
            .expect("drain 不应失败");

        let snapshot = queued_snapshot(&sink).expect("接纳后必须发布排队快照");
        assert_eq!(snapshot.len(), 3, "消息、技能、消息各占一条");
    }

    /// 单用途 Run 的用户输入回流：plain 输入生成新 InputId、accepted 输入保留原
    /// accepted 元数据与 InputId，批内顺序原样保持（下一个会话 Run 按序消费）。
    #[test]
    fn defer_user_batch_returns_events_to_session_input_preserving_order_and_identity() {
        let (mut adapter, _sink) = adapter_with(vec![]);
        let accepted_id = sdk::InputId::new_v7();
        let batch = vec![
            crate::application::loop_engine::engine::LoopInput {
                text: "plain".to_string(),
                input_id: None,
                images: Vec::new(),
                accepted: None,
            },
            crate::application::loop_engine::engine::LoopInput::accepted(
                crate::application::loop_engine::AcceptedUserInput::UserMessage {
                    input_id: accepted_id.clone(),
                    text: "accepted".to_string(),
                    images: Vec::new(),
                },
            ),
        ];

        adapter
            .defer_user_batch(batch)
            .expect("defer_user_batch 必须成功");

        let deferred = adapter.input_events.deferred.lock().unwrap().clone();
        assert_eq!(deferred.len(), 2, "整批必须全部回流");
        match &deferred[0] {
            ChatInputEvent::UserMessage { text, .. } => {
                assert_eq!(text, "plain");
            }
            other => panic!("plain 输入必须回流为 UserMessage: {other:?}"),
        }
        match &deferred[1] {
            ChatInputEvent::UserMessage { id, text, .. } => {
                assert_eq!(text, "accepted");
                assert_eq!(
                    *id, accepted_id,
                    "accepted 输入必须保留原 InputId（撤回/归属依赖它）"
                );
            }
            other => panic!("accepted 输入必须保留原事件形态: {other:?}"),
        }
    }
}

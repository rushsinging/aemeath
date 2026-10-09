use share::message::{Message, MessageSource, Role};

use std::time::Instant;

use crate::application::interaction::port::{InteractionCompletion, InteractionRequestMetadata};
use crate::application::loop_engine::PendingInteractionWork;

use tools::published::agent::AgentRunTerminal;

use crate::ports::{ContextRequestData, ContextWindowData};

pub(crate) struct ActiveInteractionReceiver {
    pub(crate) metadata: InteractionRequestMetadata,
    pub(crate) receiver: tokio::sync::oneshot::Receiver<InteractionCompletion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActiveInteractionAlreadyRegistered;

/// 产生和替换的消息、Context 投影、run step 计数与 continuation 工作集。
#[derive(Default)]
pub struct RunExecutionState {
    messages: Vec<Message>,
    accepted_input: Vec<Message>,
    pending_step_messages: Vec<Message>,
    active_step_messages: Vec<Message>,
    step_outcome: Vec<Message>,
    context_request: Option<ContextRequestData>,
    context_window: Option<ContextWindowData>,
    /// Session 级 Run 序号（用户回合计数）：Main Run 启动时由 session driver
    /// 传入（反思频控 `interval_runs` 判据、progress 回合文案）；Run 内不递增。
    /// Derived Run 恒 0（子代理 Run 不占用户回合计数）。
    run_ordinal: usize,
    /// Run 内 model invocation（step）计数：Main 与 Derived 均在 step 边界
    /// 递增（`accept_step_input` 统一推进）；用于 run_steps 终态展示、
    /// invoke 日志与 reminder OnStepInterval 推进。
    step_ordinal: usize,
    started_at: Option<Instant>,
    step_started_at: Option<Instant>,
    terminal: Option<AgentRunTerminal>,
    pending_interaction_work: Option<PendingInteractionWork>,
    adopted_input: Vec<(sdk::InputId, Message)>,
    active_interaction: Option<ActiveInteractionReceiver>,
    /// Interval 反思的 Run 级一次性闸门：主会话 Run 内 `run_ordinal` 不递增，
    /// 多个 `ModelStep::Complete`（含内部 continuation）会命中同一序号，
    /// 因此 Interval 反思在同 Run 内至多开始一次。命中并开始 phase 时消耗，
    /// 未命中不消耗；与消息/step 临时状态不同，本字段跨 `begin_step` 保留。
    interval_reflection_started: bool,
}

impl RunExecutionState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn initialize_for_launch(&mut self, messages: Vec<Message>, run_ordinal: usize) {
        debug_assert!(
            self.started_at.is_none(),
            "execution state initialized twice"
        );
        debug_assert!(
            self.messages.is_empty(),
            "execution messages initialized twice"
        );
        self.messages = messages;
        self.run_ordinal = run_ordinal;
        self.started_at = Some(Instant::now());
    }

    pub(crate) fn messages(&self) -> &[Message] {
        &self.messages
    }

    #[cfg(test)]
    pub(crate) fn accepted_input(&self) -> &[Message] {
        &self.accepted_input
    }

    #[cfg(test)]
    pub(crate) fn replace_accepted_input(&mut self, messages: Vec<Message>) {
        self.accepted_input = messages;
    }

    #[cfg(test)]
    pub(crate) fn clear_accepted_input(&mut self) {
        self.accepted_input.clear();
    }

    pub(crate) fn append_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    pub(crate) fn accept_user_messages_from(&mut self, messages: &[Message]) {
        self.accepted_input = Self::accepted_user_messages_from(messages);
    }

    pub(crate) fn accepted_user_messages_from(messages: &[Message]) -> Vec<Message> {
        messages
            .iter()
            .filter(|message| {
                message.role == Role::User
                    && message.metadata.as_ref().is_none_or(|metadata| {
                        !matches!(
                            metadata.source,
                            MessageSource::SystemGenerated | MessageSource::Hook
                        )
                    })
            })
            .cloned()
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn replace_pending_step_messages(&mut self, messages: Vec<Message>) {
        self.pending_step_messages = messages;
    }

    pub(crate) fn freeze_step_input_messages(
        &mut self,
        prefix: Option<Message>,
        inputs: Vec<Message>,
    ) -> Vec<Message> {
        let mut messages = prefix.into_iter().collect::<Vec<_>>();
        if inputs.is_empty() {
            messages.extend(std::mem::take(&mut self.pending_step_messages));
        } else {
            messages.extend(inputs);
        }
        self.freeze_step_messages(messages.clone());
        self.accept_user_messages_from(&messages);
        messages
    }

    pub(crate) fn freeze_step_messages(&mut self, messages: Vec<Message>) {
        self.active_step_messages = messages;
        self.step_outcome.clear();
    }

    pub(crate) fn record_step_message(&mut self, message: Message) {
        self.active_step_messages.push(message.clone());
        self.step_outcome.push(message);
    }

    pub(crate) fn step_outcome(&self) -> Vec<Message> {
        self.step_outcome.clone()
    }

    pub(crate) fn commit_step_messages(&mut self) {
        self.active_step_messages.clear();
        self.step_outcome.clear();
    }

    pub(crate) fn extend_messages(&mut self, messages: impl IntoIterator<Item = Message>) {
        self.messages.extend(messages);
    }

    pub(crate) fn messages_snapshot(&self) -> Vec<Message> {
        self.messages.clone()
    }

    pub(crate) fn messages_len(&self) -> usize {
        self.messages.len()
    }

    pub(crate) fn message_tokens(&self) -> usize {
        context::compact::estimate_messages_tokens(&self.messages)
    }

    pub(crate) fn accepted_input_snapshot(&self) -> Vec<Message> {
        self.accepted_input.clone()
    }

    #[cfg(test)]
    pub(crate) fn pending_step_messages_len(&self) -> usize {
        self.pending_step_messages.len()
    }

    #[cfg(test)]
    pub(crate) fn active_step_messages_len(&self) -> usize {
        self.active_step_messages.len()
    }

    pub(crate) fn context_request(&self) -> Option<&ContextRequestData> {
        self.context_request.as_ref()
    }

    pub(crate) fn context_window(&self) -> Option<&ContextWindowData> {
        self.context_window.as_ref()
    }

    pub(crate) fn context_window_mut(&mut self) -> &mut Option<ContextWindowData> {
        &mut self.context_window
    }

    pub(crate) fn replace_context_state(
        &mut self,
        request: ContextRequestData,
        window: Option<ContextWindowData>,
    ) {
        self.context_request = Some(request);
        self.context_window = window;
    }

    pub(crate) fn started_at(&self) -> Option<Instant> {
        self.started_at
    }

    pub(crate) fn elapsed(&self) -> std::time::Duration {
        self.started_at
            .map(|started_at| started_at.elapsed())
            .unwrap_or_default()
    }

    pub(crate) fn step_elapsed(&self) -> Option<std::time::Duration> {
        self.step_started_at.map(|started_at| started_at.elapsed())
    }

    /// Session 级 Run 序号（Main：用户回合计数；Derived 恒 0）。
    pub(crate) fn run_ordinal(&self) -> usize {
        self.run_ordinal
    }

    /// Run 内 step（model invocation）计数。
    pub(crate) fn step_ordinal(&self) -> usize {
        self.step_ordinal
    }

    /// Interval 反思的 Run 级一次性闸门是否已消耗（本 Run 已开始过 Interval phase）。
    pub(crate) fn interval_reflection_started(&self) -> bool {
        self.interval_reflection_started
    }

    /// 判定命中、即将进入 Interval reflection phase 时消耗本 Run 的一次性闸门。
    ///
    /// 幂等；仅在命中后调用（未命中不得消耗）。phase 内端口错误、任务失败或取消
    /// 都不回滚——该 Run 已执行过反思，后续 Complete step 不得再判定重复反思。
    pub(crate) fn mark_interval_reflection_started(&mut self) {
        self.interval_reflection_started = true;
    }

    /// 推进 Run 内 step 计数（step 边界统一调用：Main 与 Derived 同源）。
    pub(crate) fn advance_step_ordinal(&mut self) -> usize {
        self.step_ordinal += 1;
        self.step_ordinal
    }

    pub(crate) fn terminal_mut(&mut self) -> &mut Option<AgentRunTerminal> {
        &mut self.terminal
    }

    #[cfg(test)]
    pub(crate) fn set_terminal(&mut self, terminal: AgentRunTerminal) {
        self.terminal = Some(terminal);
    }

    pub(crate) fn take_terminal(&mut self) -> Option<AgentRunTerminal> {
        self.terminal.take()
    }

    pub(crate) fn replace_adopted_input(&mut self, adopted: Vec<(sdk::InputId, Message)>) {
        self.adopted_input = adopted;
    }

    #[cfg(test)]
    pub(crate) fn adopted_input(&self) -> &[(sdk::InputId, Message)] {
        &self.adopted_input
    }

    pub(crate) fn take_adopted_input(&mut self) -> Vec<(sdk::InputId, Message)> {
        std::mem::take(&mut self.adopted_input)
    }

    #[cfg(test)]
    pub(crate) fn interaction_metadata(&self) -> Vec<InteractionRequestMetadata> {
        self.active_interaction_metadata()
            .cloned()
            .into_iter()
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn active_interaction_metadata(&self) -> Option<&InteractionRequestMetadata> {
        self.active_interaction
            .as_ref()
            .map(|active| &active.metadata)
    }

    pub(crate) fn store_interaction_receiver(
        &mut self,
        metadata: InteractionRequestMetadata,
        receiver: tokio::sync::oneshot::Receiver<InteractionCompletion>,
    ) -> Result<(), ActiveInteractionAlreadyRegistered> {
        if self.active_interaction.is_some() {
            return Err(ActiveInteractionAlreadyRegistered);
        }
        self.active_interaction = Some(ActiveInteractionReceiver { metadata, receiver });
        Ok(())
    }

    pub(crate) fn take_active_interaction(&mut self) -> Option<ActiveInteractionReceiver> {
        self.active_interaction.take()
    }

    #[cfg(test)]
    pub(crate) fn pending_interaction_work(&self) -> Option<&PendingInteractionWork> {
        self.pending_interaction_work.as_ref()
    }

    pub(crate) fn set_pending_interaction_work(&mut self, work: PendingInteractionWork) {
        self.pending_interaction_work = Some(work);
    }

    pub(crate) fn take_pending_interaction_work(&mut self) -> Option<PendingInteractionWork> {
        self.pending_interaction_work.take()
    }

    /// 开始下一 Step 时清除只属于上一 Step 的临时工作集。
    /// 已提交历史消息和 Run 级 run step 计数继续保留。
    pub(crate) fn begin_step(&mut self) {
        self.accepted_input.clear();
        self.context_request = None;
        self.context_window = None;
        self.pending_interaction_work = None;
        self.step_started_at = Some(Instant::now());
    }
}

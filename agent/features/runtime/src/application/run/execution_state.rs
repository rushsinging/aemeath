use share::message::{Message, MessageSource, Role};

use std::time::Instant;

use crate::application::interaction::port::{InteractionCompletion, InteractionRequestMetadata};
use crate::application::loop_engine::PendingInteractionWork;

use tools::AgentRunTerminal;

use crate::ports::{ContextRequest, ContextWindow};

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
    context_request: Option<ContextRequest>,
    context_window: Option<ContextWindow>,
    step_count: usize,
    started_at: Option<Instant>,
    step_started_at: Option<Instant>,
    terminal: Option<AgentRunTerminal>,
    pending_interaction_work: Option<PendingInteractionWork>,
    adopted_input: Vec<(sdk::InputId, Message)>,
    active_interaction: Option<ActiveInteractionReceiver>,
}

impl RunExecutionState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn initialize_for_launch(&mut self, messages: Vec<Message>, step_count: usize) {
        debug_assert!(
            self.started_at.is_none(),
            "execution state initialized twice"
        );
        debug_assert!(
            self.messages.is_empty(),
            "execution messages initialized twice"
        );
        self.messages = messages;
        self.step_count = step_count;
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

    pub(crate) fn context_request(&self) -> Option<&ContextRequest> {
        self.context_request.as_ref()
    }

    pub(crate) fn context_window(&self) -> Option<&ContextWindow> {
        self.context_window.as_ref()
    }

    pub(crate) fn context_window_mut(&mut self) -> &mut Option<ContextWindow> {
        &mut self.context_window
    }

    pub(crate) fn replace_context_state(
        &mut self,
        request: ContextRequest,
        window: Option<ContextWindow>,
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

    pub(crate) fn step_count(&self) -> usize {
        self.step_count
    }

    pub(crate) fn advance_step(&mut self) -> usize {
        self.step_count += 1;
        self.step_count
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

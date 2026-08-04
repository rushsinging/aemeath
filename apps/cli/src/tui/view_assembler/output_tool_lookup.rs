use crate::tui::model::conversation::ids::{ChatId, ChatRunId, ToolCallId};
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::ToolCall;

pub(super) trait ToolCallLookup {
    fn call<'a>(
        &'a self,
        chat_id: &ChatId,
        run_id: &ChatRunId,
        tool_id: &ToolCallId,
    ) -> Option<&'a ToolCall>;
}

pub(super) struct ConversationToolLookup<'a> {
    conversation: &'a ConversationModel,
}

impl<'a> ConversationToolLookup<'a> {
    pub(super) fn new(conversation: &'a ConversationModel) -> Self {
        Self { conversation }
    }
}

impl ToolCallLookup for ConversationToolLookup<'_> {
    fn call<'a>(
        &'a self,
        chat_id: &ChatId,
        run_id: &ChatRunId,
        tool_id: &ToolCallId,
    ) -> Option<&'a ToolCall> {
        self.conversation.tool_call(chat_id, run_id, tool_id)
    }
}

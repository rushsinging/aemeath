use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::ToolCall;

pub(super) trait ToolCallLookup {
    fn call<'a>(
        &'a self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
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
        turn_id: &ChatTurnId,
        tool_id: &ToolCallId,
    ) -> Option<&'a ToolCall> {
        self.conversation
            .chats
            .iter()
            .find(|chat| &chat.id == chat_id)?
            .turns
            .iter()
            .find(|turn| &turn.id == turn_id)?
            .tool_calls
            .iter()
            .find(|call| call.id.as_ref() == Some(tool_id))
    }
}

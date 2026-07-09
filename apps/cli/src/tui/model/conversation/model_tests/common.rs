use super::change::ConversationChange;
use super::model::ConversationModel;
use super::tool_call::ToolCallStatus;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::output_timeline::OutputTimelineItem;

fn tool_call<'a>(
    model: &'a ConversationModel,
    chat_id: &super::ids::ChatId,
    turn_id: &super::ids::ChatTurnId,
    id: &super::ids::ToolCallId,
) -> Option<&'a super::tool_call::ToolCall> {
    model
        .chats
        .iter()
        .find(|chat| &chat.id == chat_id)
        .and_then(|chat| chat.turns.iter().find(|turn| &turn.id == turn_id))
        .and_then(|turn| {
            turn.tool_calls
                .iter()
                .find(|call| call.id.as_ref() == Some(id))
        })
}

fn timeline_tool_call_ref_exists(
    model: &ConversationModel,
    chat_id: &super::ids::ChatId,
    turn_id: &super::ids::ChatTurnId,
    id: &super::ids::ToolCallId,
) -> bool {
    model.timeline.items().iter().any(|item| {
        matches!(
            item,
            OutputTimelineItem::ToolCall { reference }
                if &reference.context.chat_id == chat_id
                    && &reference.context.turn_id == turn_id
                    && reference.tool_call_id == *id
        )
    })
}

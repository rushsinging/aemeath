use super::OutputViewAssembler;
use crate::tui::model::conversation::ids::ToolCallId;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::render::output::rendered::RenderCtx;
use crate::tui::view_model::{OutputBlockKind, ToolSemanticStatus};


fn add_failed_tool_after_thinking(conversation: &mut ConversationModel, name: &str, output: &str) {
    add_tool_after_thinking(conversation, name, output, true);
}

fn add_completed_tool_after_thinking(
    conversation: &mut ConversationModel,
    name: &str,
    output: &str,
) {
    add_tool_after_thinking(conversation, name, output, false);
}

fn add_tool_after_thinking(
    conversation: &mut ConversationModel,
    name: &str,
    output: &str,
    is_error: bool,
) {
    conversation.apply(StartChat {
        submission: "search".to_string(),
    });
    conversation.apply(ThinkingText {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        text: "thinking".to_string(),
    });
    conversation.apply(CompleteBlock {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
    });
    add_completed_tool(
        conversation,
        "tool-1",
        name,
        "search docs",
        output,
        is_error,
    );
}

fn add_completed_tool(
    conversation: &mut ConversationModel,
    id: &str,
    name: &str,
    _summary: &str,
    output: &str,
    is_error: bool,
) {
    conversation.apply(ToolCallStart {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        id: ToolCallId::new(id),
        provider_id: None,
        name: name.to_string(),
        index: 0,
    });
    conversation.apply(ToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        provider_id: Some(format!("provider-{id}")),
        id: ToolCallId::new(id),
        name: name.to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    conversation.apply(ToolResult {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        provider_id: format!("provider-{id}"),
        id: ToolCallId::new(id),
        tool_name: name.to_string(),
        output: output.to_string(),
        content: serde_json::json!({ "text": output }),
        is_error,
        image_count: 0,
    });
}

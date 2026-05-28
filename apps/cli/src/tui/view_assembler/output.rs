use crate::tui::model::conversation::block::ConversationBlock;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::view_model::{
    OutputBlockView, OutputViewModel, SemanticStyle, TextBlockView, ToolCallBlockView,
    ToolSemanticStatus,
};

pub struct OutputViewAssembler;

impl OutputViewAssembler {
    pub fn assemble_from_conversation(
        conversation: &ConversationModel,
        version: u64,
    ) -> OutputViewModel {
        let mut blocks = Vec::new();
        for conversation_block in &conversation.blocks {
            match conversation_block {
                ConversationBlock::UserMessage { id, text } => {
                    blocks.push(OutputBlockView::UserMessage(TextBlockView {
                        key: id.clone(),
                        text: text.clone(),
                        style: SemanticStyle::Normal,
                    }));
                }
                ConversationBlock::AssistantText { id, text } => {
                    blocks.push(OutputBlockView::AssistantMessage(TextBlockView {
                        key: id.clone(),
                        text: text.clone(),
                        style: SemanticStyle::Normal,
                    }));
                }
                ConversationBlock::Thinking { id, text } => {
                    blocks.push(OutputBlockView::AssistantMessage(TextBlockView {
                        key: id.clone(),
                        text: text.clone(),
                        style: SemanticStyle::Muted,
                    }));
                }
                ConversationBlock::ToolCall { id, .. } => {
                    if let Some(tool) = find_tool_view(conversation, id.as_ref()) {
                        blocks.push(OutputBlockView::ToolCall(tool));
                    }
                }
                ConversationBlock::ToolResult {
                    id,
                    output,
                    is_error,
                    image_count,
                } => {
                    let mut text = output.clone();
                    if *image_count > 0 {
                        text.push_str(&format!("\n[图片: {image_count}]").to_string());
                    }
                    blocks.push(OutputBlockView::DiagnosticNotice(TextBlockView {
                        key: format!("{}-result", id.as_ref()),
                        text,
                        style: if *is_error {
                            SemanticStyle::Error
                        } else {
                            SemanticStyle::Success
                        },
                    }));
                }
                ConversationBlock::System { id, text } => {
                    blocks.push(OutputBlockView::SystemNotice(TextBlockView {
                        key: id.clone(),
                        text: text.clone(),
                        style: SemanticStyle::Muted,
                    }));
                }
                ConversationBlock::Error { id, text } => {
                    blocks.push(OutputBlockView::DiagnosticNotice(TextBlockView {
                        key: id.clone(),
                        text: text.clone(),
                        style: SemanticStyle::Error,
                    }));
                }
                ConversationBlock::QueuedUserMessage { id, text } => {
                    blocks.push(OutputBlockView::UserMessage(TextBlockView {
                        key: id.clone(),
                        text: format!("排队中: {text}"),
                        style: SemanticStyle::Muted,
                    }));
                }
                ConversationBlock::AgentProgress {
                    id,
                    tool_id,
                    message,
                } => {
                    blocks.push(OutputBlockView::DiagnosticNotice(TextBlockView {
                        key: id.clone(),
                        text: format!("{tool_id}: {message}"),
                        style: SemanticStyle::Running,
                    }));
                }
                ConversationBlock::OrphanToolResult {
                    id,
                    output,
                    is_error,
                } => {
                    blocks.push(OutputBlockView::DiagnosticNotice(TextBlockView {
                        key: format!("orphan-{id}"),
                        text: output.clone(),
                        style: if *is_error {
                            SemanticStyle::Error
                        } else {
                            SemanticStyle::Warning
                        },
                    }));
                }
            }
        }
        OutputViewModel {
            blocks,
            version,
            follow_tail_hint: true,
        }
    }
}

fn find_tool_view(conversation: &ConversationModel, tool_id: &str) -> Option<ToolCallBlockView> {
    for chat in &conversation.chats {
        for turn in &chat.turns {
            for call in &turn.tool_calls {
                if call.id.as_ref().map(|id| id.as_ref()) != Some(tool_id) {
                    continue;
                }
                let (icon, semantic_status, style) = map_tool_status(call.status);
                return Some(ToolCallBlockView {
                    key: format!("{}/{}/{}", chat.id.as_ref(), turn.id.as_ref(), tool_id),
                    chat_id: Some(chat.id.as_ref().to_string()),
                    turn_id: Some(turn.id.as_ref().to_string()),
                    tool_call_id: Some(tool_id.to_string()),
                    title: call.name.clone(),
                    icon: icon.to_string(),
                    semantic_status,
                    style,
                    args_preview: (!call.args_preview.is_empty())
                        .then(|| call.args_preview.clone()),
                    summary: call.summary.clone(),
                    activity_summary: call.activities.last().cloned(),
                    result_summary: call.result.clone(),
                    collapsible: true,
                    collapsed: false,
                });
            }
        }
    }
    None
}

fn map_tool_status(status: ToolCallStatus) -> (&'static str, ToolSemanticStatus, SemanticStyle) {
    match status {
        ToolCallStatus::PendingArgs | ToolCallStatus::Ready | ToolCallStatus::Running => {
            ("●", ToolSemanticStatus::Running, SemanticStyle::Running)
        }
        ToolCallStatus::Success => ("✓", ToolSemanticStatus::Success, SemanticStyle::Success),
        ToolCallStatus::Error => ("✗", ToolSemanticStatus::Error, SemanticStyle::Error),
        ToolCallStatus::Cancelled => ("–", ToolSemanticStatus::Cancelled, SemanticStyle::Muted),
        ToolCallStatus::Orphaned => ("?", ToolSemanticStatus::Orphaned, SemanticStyle::Warning),
    }
}

#[cfg(test)]
mod tests {
    use crate::tui::model::conversation::intent::ConversationIntent;
    use crate::tui::model::conversation::model::ConversationModel;
    use crate::tui::view_model::{OutputBlockView, ToolSemanticStatus};

    use super::OutputViewAssembler;

    #[test]
    fn test_output_assembler_maps_tool_status_to_icon() {
        let mut conversation = ConversationModel::default();
        conversation.apply(ConversationIntent::StartChat {
            submission: "read".to_string(),
        });
        conversation.apply(ConversationIntent::ObserveToolCallStart {
            name: "Read".to_string(),
            index: 0,
        });
        conversation.apply(ConversationIntent::ObserveToolCall {
            id: "tool-1".to_string(),
            name: "Read".to_string(),
            index: 0,
            summary: "Read file".to_string(),
        });
        conversation.apply(ConversationIntent::ObserveToolResult {
            id: "tool-1".to_string(),
            tool_name: "Read".to_string(),
            output: "ok".to_string(),
            is_error: false,
            image_count: 0,
        });

        let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
        let tool = vm
            .blocks
            .iter()
            .find_map(|block| match block {
                OutputBlockView::ToolCall(tool) => Some(tool),
                _ => None,
            })
            .expect("tool block");

        assert_eq!(tool.icon, "✓");
        assert_eq!(tool.semantic_status, ToolSemanticStatus::Success);
    }
}

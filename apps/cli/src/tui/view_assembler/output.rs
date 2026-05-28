use crate::tui::model::conversation::{ConversationModel, ToolCallStatus};
use crate::tui::output_area::{LineStyle, OutputArea};
use crate::tui::view_model::{
    OutputBlockView, OutputViewModel, SemanticStyle, TextBlockView, ToolCallBlockView,
    ToolSemanticStatus,
};

pub struct OutputViewAssembler;

impl OutputViewAssembler {
    pub fn assemble_from_output_area(output: &OutputArea, version: u64) -> OutputViewModel {
        let blocks = output
            .lines
            .iter()
            .enumerate()
            .map(|(idx, line)| {
                let style = match line.style {
                    LineStyle::Error | LineStyle::ToolCallError => SemanticStyle::Error,
                    LineStyle::ToolCallSuccess => SemanticStyle::Success,
                    LineStyle::ToolCallRunning => SemanticStyle::Running,
                    LineStyle::System | LineStyle::Thinking => SemanticStyle::Muted,
                    _ => SemanticStyle::Normal,
                };
                OutputBlockView::SystemNotice(TextBlockView {
                    key: format!("legacy-line-{idx}"),
                    text: line.content.clone(),
                    style,
                })
            })
            .collect();
        OutputViewModel {
            blocks,
            version,
            follow_tail_hint: output.auto_scroll,
        }
    }

    pub fn assemble_from_conversation(
        conversation: &ConversationModel,
        version: u64,
    ) -> OutputViewModel {
        let mut blocks = Vec::new();
        for chat in &conversation.chats {
            for turn in &chat.turns {
                for call in &turn.tool_calls {
                    let (icon, semantic_status, style) = map_tool_status(call.status);
                    blocks.push(OutputBlockView::ToolCall(ToolCallBlockView {
                        key: format!(
                            "{}/{}/{}",
                            chat.id.as_ref(),
                            turn.id.as_ref(),
                            call.id
                                .as_ref()
                                .map(AsRef::as_ref)
                                .unwrap_or(call.name.as_str())
                        ),
                        chat_id: Some(chat.id.as_ref().to_string()),
                        turn_id: Some(turn.id.as_ref().to_string()),
                        tool_call_id: call.id.as_ref().map(|id| id.as_ref().to_string()),
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
    use crate::tui::model::conversation::{ConversationIntent, ConversationModel};
    use crate::tui::output_area::{LineStyle, OutputArea};
    use crate::tui::view_model::{OutputBlockView, ToolSemanticStatus};

    use super::OutputViewAssembler;

    #[test]
    fn test_output_assembler_converts_existing_lines_to_blocks() {
        let mut output = OutputArea::new();
        output.push_system("hello");
        let vm = OutputViewAssembler::assemble_from_output_area(&output, 1);
        assert_eq!(vm.version, 1);
        assert_eq!(vm.blocks.len(), 1);
        assert!(matches!(
            output.lines.front().map(|line| line.style),
            Some(LineStyle::System)
        ));
    }

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
            output: "ok".to_string(),
            is_error: false,
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

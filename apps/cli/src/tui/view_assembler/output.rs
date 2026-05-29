use crate::tui::model::conversation::block::ConversationBlock;
use crate::tui::model::conversation::ids::ToolCallId;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::render::output::tool_display::lookup_display;
use crate::tui::view_model::{
    AskUserBlockView, OutputBlockKind, OutputBlockView, OutputViewModel, SemanticStyle,
    TextBlockView, ToolCallBlockView, ToolSemanticStatus,
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
                    blocks.push(output_block(
                        id.clone(),
                        OutputBlockKind::UserMessage(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Normal,
                        }),
                    ));
                }
                ConversationBlock::AssistantText { id, text } => {
                    blocks.push(output_block(
                        id.clone(),
                        OutputBlockKind::AssistantMessage(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Normal,
                        }),
                    ));
                }
                ConversationBlock::Thinking { id, text } => {
                    blocks.push(output_block(
                        id.clone(),
                        OutputBlockKind::ThinkingMessage(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Muted,
                        }),
                    ));
                }
                ConversationBlock::ToolCall { id, .. } => {
                    if let Some(tool) = find_tool_view(conversation, id.as_ref()) {
                        blocks.push(output_block(
                            tool.key.clone(),
                            OutputBlockKind::ToolCall(tool),
                        ));
                    }
                }
                ConversationBlock::ToolResult {
                    id,
                    output,
                    is_error,
                    image_count,
                } => {
                    if tool_result_is_embedded(conversation, id) {
                        continue;
                    }
                    let mut text = output.clone();
                    if *image_count > 0 {
                        text.push_str(&format!("\n[图片: {image_count}]").to_string());
                    }
                    blocks.push(output_block(
                        format!("{}-result", id.as_ref()),
                        OutputBlockKind::DiagnosticNotice(TextBlockView {
                            key: format!("{}-result", id.as_ref()),
                            text,
                            style: if *is_error {
                                SemanticStyle::Error
                            } else {
                                SemanticStyle::Success
                            },
                        }),
                    ));
                }
                ConversationBlock::System { id, text } => {
                    blocks.push(output_block(
                        id.clone(),
                        OutputBlockKind::SystemNotice(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Muted,
                        }),
                    ));
                }
                ConversationBlock::Error { id, text } => {
                    blocks.push(output_block(
                        id.clone(),
                        OutputBlockKind::DiagnosticNotice(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Error,
                        }),
                    ));
                }
                ConversationBlock::QueuedUserMessage { id, text } => {
                    blocks.push(output_block(
                        id.clone(),
                        OutputBlockKind::QueuedSubmission(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Muted,
                        }),
                    ));
                }
                ConversationBlock::AgentProgress {
                    id,
                    tool_id,
                    message,
                } => {
                    blocks.push(output_block(
                        id.clone(),
                        OutputBlockKind::DiagnosticNotice(TextBlockView {
                            key: id.clone(),
                            text: format!("{tool_id}: {message}"),
                            style: SemanticStyle::Running,
                        }),
                    ));
                }
                ConversationBlock::AskUser {
                    id,
                    question,
                    options,
                    llm_option_count,
                    multi_select,
                    cursor,
                    selected,
                    chat_input_active,
                    default,
                } => {
                    blocks.push(output_block(
                        id.clone(),
                        OutputBlockKind::AskUser(AskUserBlockView {
                            key: id.clone(),
                            question: question.clone(),
                            options: options.clone(),
                            llm_option_count: *llm_option_count,
                            multi_select: *multi_select,
                            cursor: *cursor,
                            selected: selected.clone(),
                            chat_input_active: *chat_input_active,
                            default: default.clone(),
                        }),
                    ));
                }
                ConversationBlock::OrphanToolResult {
                    id,
                    output,
                    is_error,
                } => {
                    blocks.push(output_block(
                        format!("orphan-{id}"),
                        OutputBlockKind::DiagnosticNotice(TextBlockView {
                            key: format!("orphan-{id}"),
                            text: output.clone(),
                            style: if *is_error {
                                SemanticStyle::Error
                            } else {
                                SemanticStyle::Warning
                            },
                        }),
                    ));
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

fn output_block(block_id: String, kind: OutputBlockKind) -> OutputBlockView {
    let block_version = kind.cache_version();
    OutputBlockView {
        block_id,
        block_version,
        kind,
    }
}

fn tool_result_is_embedded(conversation: &ConversationModel, tool_id: &ToolCallId) -> bool {
    conversation.chats.iter().any(|chat| {
        chat.turns.iter().any(|turn| {
            turn.tool_calls.iter().any(|call| {
                call.id.as_ref() == Some(tool_id)
                    && call
                        .result
                        .as_ref()
                        .is_some_and(|result| !result.is_empty())
            })
        })
    })
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
                    result_summary: tool_result_summary(
                        &call.name,
                        call.result.as_deref(),
                        call.status,
                    ),
                    collapsible: true,
                    collapsed: false,
                });
            }
        }
    }
    None
}

fn tool_result_summary(
    tool_name: &str,
    result: Option<&str>,
    status: ToolCallStatus,
) -> Option<String> {
    let result = result?;
    if result.is_empty() {
        return None;
    }
    let is_error = matches!(status, ToolCallStatus::Error);
    let lines = lookup_display(tool_name)
        .map(|display| display.format_result_summary(result, is_error))
        .filter(|lines| !lines.is_empty())
        .unwrap_or_else(|| default_tool_result_summary(tool_name, is_error));
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn default_tool_result_summary(tool_name: &str, is_error: bool) -> Vec<String> {
    if is_error {
        vec![format!("✗ {tool_name} failed")]
    } else {
        vec![format!("✓ {tool_name} completed")]
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
#[path = "output_tests.rs"]
mod tests;

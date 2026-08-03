use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId, ToolStreamKey};
use crate::tui::model::conversation::resumed_history::{
    ResumedHistoryItem, ResumedHistoryItemKind, ResumedHistoryStep,
};
use crate::tui::model::conversation::tool_call::{ToolCall, ToolCallStatus};
use crate::tui::model::conversation::tool_result_payload::ToolResultPayload;
use crate::tui::model::display_history::DisplayHistoryModel;
use crate::tui::view_assembler::output_tool_lookup::ToolCallLookup;
use crate::tui::view_model::{BlockNode, OutputBlockKind, SemanticStyle, TextBlockView};
use sdk::LocalResumeContentBlock as ContentBlock;

pub(crate) fn assemble_resumed_history_item(
    display_history: &DisplayHistoryModel,
    item: &ResumedHistoryItem,
) -> Option<BlockNode> {
    let step = display_history.step(item.step_index)?;
    match item.kind {
        ResumedHistoryItemKind::UserMessage { message_index } => text_leaf(
            item.id.clone(),
            OutputBlockKind::UserMessage(TextBlockView {
                key: item.id.clone(),
                text: step.message(message_index)?.text_content(),
                style: SemanticStyle::Normal,
            }),
        ),
        ResumedHistoryItemKind::AssistantText {
            message_index,
            block_index,
        } => match step.message(message_index)?.content.get(block_index)? {
            ContentBlock::Text { text } => text_leaf(
                item.id.clone(),
                OutputBlockKind::AssistantMessage(TextBlockView {
                    key: item.id.clone(),
                    text: text.clone(),
                    style: SemanticStyle::Normal,
                }),
            ),
            _ => None,
        },
        ResumedHistoryItemKind::Thinking {
            message_index,
            block_index,
        } => match step.message(message_index)?.content.get(block_index)? {
            ContentBlock::Thinking { thinking, .. } => text_leaf(
                item.id.clone(),
                OutputBlockKind::ThinkingMessage(TextBlockView {
                    key: item.id.clone(),
                    text: thinking.clone(),
                    style: SemanticStyle::Muted,
                }),
            ),
            _ => None,
        },
        ResumedHistoryItemKind::ToolCall { .. } | ResumedHistoryItemKind::ToolResult { .. } => {
            materialize_tool_item(step, item)
        }
        ResumedHistoryItemKind::TypedJson { ref text, .. } => text_leaf(
            item.id.clone(),
            OutputBlockKind::SystemNotice(TextBlockView {
                key: item.id.clone(),
                text: text.clone(),
                style: SemanticStyle::Muted,
            }),
        ),
        ResumedHistoryItemKind::StepPlaceholder => None,
        ResumedHistoryItemKind::TerminalNotice => {
            let text = terminal_text(step.finalize_cause?, step.duration_ms);
            text_leaf(
                item.id.clone(),
                OutputBlockKind::SystemNotice(TextBlockView {
                    key: item.id.clone(),
                    text,
                    style: SemanticStyle::Muted,
                }),
            )
        }
    }
}

fn materialize_tool_item(
    step: &ResumedHistoryStep,
    item: &ResumedHistoryItem,
) -> Option<BlockNode> {
    let (message_index, block_index, result_item) = match item.kind {
        ResumedHistoryItemKind::ToolCall {
            message_index,
            block_index,
        } => (message_index, block_index, false),
        ResumedHistoryItemKind::ToolResult {
            message_index,
            block_index,
        } => (message_index, block_index, true),
        _ => return None,
    };
    let block = step.message(message_index)?.content.get(block_index)?;
    let (provider_id, tool_name, tool_input) = match block {
        ContentBlock::ToolUse { id, name, input } => (id.as_str(), name.as_str(), input),
        ContentBlock::ToolResult { tool_use_id, .. } => {
            let (name, input) = find_tool_use(step, tool_use_id)?;
            (tool_use_id.as_str(), name, input)
        }
        _ => return None,
    };
    let result = find_tool_result(step, provider_id);
    let tool_id = ToolCallId::from_legacy_or_new(provider_id);
    let mut call = ToolCall::pending(
        tool_id.clone(),
        ToolStreamKey::new(
            ChatId::from_legacy_or_new(&step.run_id),
            ChatTurnId::from_legacy_or_new(&step.step_id),
            tool_name,
            block_index,
        ),
    );
    call.update_args(tool_input.to_string());
    call.status = if let Some((content, is_error, text)) = result {
        let output = text
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                crate::tui::model::conversation::history_parse::tool_result_content_to_string(
                    content,
                )
            });
        call.complete(ToolResultPayload::new(
            output,
            crate::tui::model::conversation::history_parse::normalize_tool_result_content(content),
            is_error,
            crate::tui::model::conversation::history_parse::tool_result_image_count(content),
        ));
        if is_error {
            ToolCallStatus::Error
        } else {
            ToolCallStatus::Success
        }
    } else {
        ToolCallStatus::Cancelled
    };
    let lookup = ResumedToolLookup {
        chat_id: &call.stream_key.chat_id,
        turn_id: &call.stream_key.turn_id,
        tool_id: &tool_id,
        call: &call,
    };
    let source_item = if result_item {
        crate::tui::model::output_timeline::OutputTimelineItem::ToolResult {
            reference: crate::tui::model::output_timeline::TimelineToolCallRef::new(
                call.stream_key.chat_id.clone(),
                call.stream_key.turn_id.clone(),
                tool_id.clone(),
            ),
        }
    } else {
        crate::tui::model::output_timeline::OutputTimelineItem::ToolCall {
            reference: crate::tui::model::output_timeline::TimelineToolCallRef::new(
                call.stream_key.chat_id.clone(),
                call.stream_key.turn_id.clone(),
                tool_id.clone(),
            ),
        }
    };
    super::output::OutputViewAssembler::assemble_item(&source_item, &lookup, None)
}

fn find_tool_use<'a>(
    step: &'a ResumedHistoryStep,
    provider_id: &str,
) -> Option<(&'a str, &'a serde_json::Value)> {
    step.messages()
        .flat_map(|message| message.content.iter())
        .find_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } if id == provider_id => {
                Some((name.as_str(), input))
            }
            _ => None,
        })
}

fn find_tool_result<'a>(
    step: &'a ResumedHistoryStep,
    provider_id: &str,
) -> Option<(&'a serde_json::Value, bool, Option<&'a str>)> {
    step.messages()
        .flat_map(|message| message.content.iter())
        .find_map(|block| match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
                text,
            } if tool_use_id == provider_id => Some((content, *is_error, text.as_deref())),
            _ => None,
        })
}

struct ResumedToolLookup<'a> {
    chat_id: &'a ChatId,
    turn_id: &'a ChatTurnId,
    tool_id: &'a ToolCallId,
    call: &'a ToolCall,
}

impl ToolCallLookup for ResumedToolLookup<'_> {
    fn call<'a>(
        &'a self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
        tool_id: &ToolCallId,
    ) -> Option<&'a ToolCall> {
        (self.chat_id == chat_id && self.turn_id == turn_id && self.tool_id == tool_id)
            .then_some(self.call)
    }
}

fn terminal_text(
    cause: crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause,
    duration_ms: Option<u64>,
) -> String {
    let status = match cause {
        crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::Completed => "✓ Completed",
        crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::UserCancelledStep => {
            "已取消"
        }
        crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::RunTerminated => {
            "此 Run 已终止"
        }
    };
    match duration_ms {
        Some(duration_ms) => format!("{status} ({:.1}s)", duration_ms as f64 / 1000.0),
        None => status.to_string(),
    }
}

fn text_leaf(block_id: String, kind: OutputBlockKind) -> Option<BlockNode> {
    let block_version = kind.cache_version();
    Some(BlockNode {
        block_id,
        block_version,
        kind,
        children: Vec::new(),
    })
}

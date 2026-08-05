use crate::tui::model::conversation::ids::{ChatId, ChatRunId, ToolCallId, ToolStreamKey};
use crate::tui::model::conversation::resumed_history::{
    ResumedHistoryItem, ResumedHistoryItemKind, ResumedHistoryStep, TypedJsonHistorySource,
};
use crate::tui::model::conversation::tool_call::{ToolCall, ToolCallStatus};
use crate::tui::model::conversation::tool_result_payload::ToolResultPayload;
use crate::tui::model::display_history::DisplayHistoryModel;
use crate::tui::view_assembler::output_tool_lookup::ToolCallLookup;
use crate::tui::view_model::output::HookNoticeBlockView;
use crate::tui::view_model::{BlockNode, OutputBlockKind, SemanticStyle, TextBlockView};
use sdk::LocalResumeContentBlock as ContentBlock;

pub(crate) fn resumed_history_candidate(
    display_history: &DisplayHistoryModel,
    item: &ResumedHistoryItem,
) -> crate::tui::view_assembler::tool_group::ToolGroupCandidate {
    let step = display_history.step(item.step_index);
    let (tool_name, tool_id, result_call_id) = step
        .and_then(|step| match item.kind {
            ResumedHistoryItemKind::ToolCall {
                message_index,
                block_index,
            } => match step.message(message_index)?.content.get(block_index)? {
                ContentBlock::ToolUse { id, name, .. } => Some((name.as_str(), id.as_str(), None)),
                _ => None,
            },
            ResumedHistoryItemKind::ToolResult {
                message_index,
                block_index,
            } => match step.message(message_index)?.content.get(block_index)? {
                ContentBlock::ToolResult { tool_use_id, .. } => {
                    Some(("", item.id.as_str(), Some(tool_use_id.as_str())))
                }
                _ => None,
            },
            _ => None,
        })
        .unwrap_or(("", item.id.as_str(), None));
    crate::tui::view_assembler::tool_group::ToolGroupCandidate {
        item_id: item.id.clone(),
        call_id: Some(tool_id.to_string()),
        tool_kind: super::tool_group::classify_tool_name(tool_name),
        step_id: step
            .map(|step| step.step_id.clone())
            .unwrap_or_else(|| item.step_index.to_string()),
        result_call_id: result_call_id.map(str::to_string),
    }
}

pub(crate) fn resumed_history_display_unit_plans(
    display_history: &DisplayHistoryModel,
) -> Vec<crate::tui::view_assembler::tool_group::DisplayUnitPlan> {
    let candidates = display_history
        .items()
        .iter()
        .map(|item| resumed_history_candidate(display_history, item))
        .collect::<Vec<_>>();
    crate::tui::view_assembler::tool_group::plan_display_units(&candidates)
}

#[cfg(test)]
pub(crate) fn assemble_resumed_history_display_units(
    display_history: &DisplayHistoryModel,
) -> Vec<BlockNode> {
    resumed_history_display_unit_plans(display_history)
        .iter()
        .filter_map(|unit| assemble_resumed_history_display_unit(display_history, unit))
        .collect()
}

pub(crate) fn assemble_resumed_history_display_unit(
    display_history: &DisplayHistoryModel,
    unit: &crate::tui::view_assembler::tool_group::DisplayUnitPlan,
) -> Option<BlockNode> {
    let belongs_to_history = match unit {
        crate::tui::view_assembler::tool_group::DisplayUnitPlan::Single { item_id, .. } => {
            display_history.item(item_id).is_some()
        }
        crate::tui::view_assembler::tool_group::DisplayUnitPlan::ToolGroup {
            member_ids, ..
        } => member_ids.iter().any(|member_id| {
            display_history.items().iter().any(|item| {
                matches!(item.kind, ResumedHistoryItemKind::ToolCall { .. })
                    && resumed_history_candidate(display_history, item)
                        .call_id
                        .as_deref()
                        == Some(member_id)
            })
        }),
    };
    if !belongs_to_history {
        return None;
    }
    match unit {
        crate::tui::view_assembler::tool_group::DisplayUnitPlan::Single {
            item_id,
            attached_results,
        } => {
            let item = display_history.item(item_id)?;
            let mut root = assemble_resumed_history_item(display_history, item)?;
            for attached_result in attached_results {
                let result_item = display_history.item(&attached_result.item_id)?;
                let Some(result) = assemble_resumed_history_item(display_history, result_item)
                else {
                    continue;
                };
                if let Some(parent) = root.children.last_mut() {
                    if matches!(parent.kind, OutputBlockKind::ToolCall(_)) {
                        parent.children.push(result);
                    }
                }
            }
            Some(root)
        }
        crate::tui::view_assembler::tool_group::DisplayUnitPlan::ToolGroup {
            group_id,
            kind,
            member_ids,
            attached_results,
        } => {
            let mut root = text_leaf(
                group_id.clone(),
                OutputBlockKind::ToolGroup(crate::tui::view_model::output::ToolGroupBlockView {
                    key: group_id.clone(),
                    kind: *kind,
                    title: kind.title().to_string(),
                    style: SemanticStyle::Muted,
                }),
            )?;
            for member_id in member_ids {
                let item = display_history.items().iter().find(|item| {
                    resumed_history_candidate(display_history, item)
                        .call_id
                        .as_deref()
                        == Some(member_id)
                })?;
                let member = assemble_resumed_history_item(display_history, item)?;
                root.children.push(member);
            }
            for attached_result in attached_results {
                let result_item = display_history.item(&attached_result.item_id)?;
                let Some(result) = assemble_resumed_history_item(display_history, result_item)
                else {
                    continue;
                };
                let parent = root.children.iter_mut().find(|child| match &child.kind {
                    OutputBlockKind::ToolCall(tool_call) => {
                        tool_call.tool_call_id.as_deref() == Some(attached_result.call_id.as_str())
                    }
                    _ => false,
                })?;
                parent.children.push(result);
            }
            Some(root)
        }
    }
}
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
        ResumedHistoryItemKind::HookNotice {
            ref title,
            ref text,
            ref kind,
        } => text_leaf(
            item.id.clone(),
            OutputBlockKind::HookNotice(HookNoticeBlockView {
                key: item.id.clone(),
                title: title.clone(),
                body: text.clone(),
                kind: kind.clone(),
            }),
        ),
        ResumedHistoryItemKind::TypedJson {
            source: TypedJsonHistorySource::SkillRequest,
            ref text,
            ..
        } => text_leaf(
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
            ChatRunId::from_legacy_or_new(&step.step_id),
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
        run_id: &call.stream_key.run_id,
        tool_id: &tool_id,
        call: &call,
    };
    let source_item = if result_item {
        crate::tui::model::output_timeline::OutputTimelineItem::ToolResult {
            reference: crate::tui::model::output_timeline::TimelineToolCallRef::new(
                call.stream_key.chat_id.clone(),
                call.stream_key.run_id.clone(),
                tool_id.clone(),
            ),
        }
    } else {
        crate::tui::model::output_timeline::OutputTimelineItem::ToolCall {
            reference: crate::tui::model::output_timeline::TimelineToolCallRef::new(
                call.stream_key.chat_id.clone(),
                call.stream_key.run_id.clone(),
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
    run_id: &'a ChatRunId,
    tool_id: &'a ToolCallId,
    call: &'a ToolCall,
}

impl ToolCallLookup for ResumedToolLookup<'_> {
    fn call<'a>(
        &'a self,
        chat_id: &ChatId,
        run_id: &ChatRunId,
        tool_id: &ToolCallId,
    ) -> Option<&'a ToolCall> {
        (self.chat_id == chat_id && self.run_id == run_id && self.tool_id == tool_id)
            .then_some(self.call)
    }
}

fn terminal_text(
    cause: crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause,
    duration_ms: Option<u64>,
) -> String {
    use crate::tui::model::conversation::terminal::{terminal_notice, TerminalCause};

    let cause = match cause {
        crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::Completed => {
            TerminalCause::Completed
        }
        crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::UserCancelledStep => {
            TerminalCause::UserCancelled
        }
        crate::tui::adapter::runtime_view::TuiResumedStepFinalizeCause::RunTerminated => {
            TerminalCause::RunTerminated
        }
    };
    terminal_notice(cause, duration_ms.map(std::time::Duration::from_millis)).unwrap_or_default()
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

#[cfg(test)]
#[path = "resumed_history_tests.rs"]
mod tests;

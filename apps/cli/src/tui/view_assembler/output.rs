#[cfg(test)]
use crate::tui::model::conversation::ids::{ChatId, ChatRunId, ToolCallId};
#[cfg(test)]
use crate::tui::model::conversation::model::ConversationModel;
#[cfg(test)]
use crate::tui::model::conversation::tool_call::ToolCall;
use crate::tui::model::output_timeline::OutputTimelineItem;
use crate::tui::view_model::output::HookNoticeBlockView;
#[cfg(test)]
use crate::tui::view_model::OutputViewModel;
use crate::tui::view_model::{
    allowed_child, AskUserBatchBlockView, AskUserPhaseView, AskUserSlotView, BlockNode,
    OutputBlockKind, SemanticStyle, TextBlockView, ToolGroupBlockView, ToolResultBlockView,
    MAX_BLOCK_DEPTH,
};
#[cfg(test)]
use std::collections::HashMap;

use super::output_tool_lookup::ToolCallLookup;
use super::output_tool_view::{
    display_text_for_tool_result, find_tool_call, find_tool_view, summarize_non_embedded_result,
    tool_result_is_embedded,
};
use super::tool_group::DisplayUnitPlan;

#[cfg(test)]
/// 测试参考装配使用的完整工具索引。
pub(super) struct ToolIndex<'a> {
    calls: HashMap<(&'a ChatId, &'a ChatRunId, &'a ToolCallId), &'a ToolCall>,
}

#[cfg(test)]
impl<'a> ToolIndex<'a> {
    pub(super) fn build(conversation: &'a ConversationModel) -> Self {
        let mut calls = HashMap::new();
        for chat in &conversation.chats {
            for turn in &chat.runs {
                for call in &turn.tool_calls {
                    if let Some(id) = call.id.as_ref() {
                        calls.insert((&chat.id, &turn.id, id), call);
                    }
                }
            }
        }
        Self { calls }
    }

    #[cfg(test)]
    pub(super) fn call(
        &self,
        chat_id: &ChatId,
        run_id: &ChatRunId,
        tool_id: &ToolCallId,
    ) -> Option<&'a ToolCall> {
        self.calls.get(&(chat_id, run_id, tool_id)).copied()
    }
}

#[cfg(test)]
impl ToolCallLookup for ToolIndex<'_> {
    fn call<'a>(
        &'a self,
        chat_id: &ChatId,
        run_id: &ChatRunId,
        tool_id: &ToolCallId,
    ) -> Option<&'a ToolCall> {
        self.calls.get(&(chat_id, run_id, tool_id)).copied()
    }
}

pub struct OutputViewAssembler;

impl OutputViewAssembler {
    pub(super) fn timeline_display_unit_plans(
        timeline_items: &[OutputTimelineItem],
        tool_lookup: &impl ToolCallLookup,
    ) -> Vec<DisplayUnitPlan> {
        let candidates = timeline_items
            .iter()
            .map(|item| super::tool_group::timeline_candidate(item, tool_lookup))
            .collect::<Vec<_>>();
        super::tool_group::plan_display_units(&candidates)
    }

    #[cfg(test)]
    pub(super) fn assemble_timeline_display_units(
        timeline_items: &[OutputTimelineItem],
        tool_lookup: &impl ToolCallLookup,
        workspace_root: Option<&std::path::Path>,
    ) -> Vec<BlockNode> {
        Self::timeline_display_unit_plans(timeline_items, tool_lookup)
            .iter()
            .filter_map(|unit| {
                Self::assemble_display_unit(unit, timeline_items, tool_lookup, workspace_root)
            })
            .collect()
    }
    pub(super) fn assemble_display_unit(
        unit: &DisplayUnitPlan,
        timeline_items: &[OutputTimelineItem],
        tool_lookup: &impl ToolCallLookup,
        workspace_root: Option<&std::path::Path>,
    ) -> Option<BlockNode> {
        let belongs_to_timeline = match unit {
            DisplayUnitPlan::Single { item_id, .. } => timeline_items
                .iter()
                .any(|item| item.id().as_ref() == item_id),
            DisplayUnitPlan::ToolGroup { member_ids, .. } => member_ids.iter().any(|member_id| {
                timeline_items.iter().any(|item| match item {
                    OutputTimelineItem::ToolCall { reference } => {
                        reference.tool_call_id.as_ref() == member_id
                    }
                    _ => false,
                })
            }),
        };
        if !belongs_to_timeline {
            return None;
        }
        match unit {
            DisplayUnitPlan::Single {
                item_id,
                attached_results,
            } => {
                let source_item = timeline_items
                    .iter()
                    .find(|item| item.id().as_ref() == item_id)?;
                let mut root = Self::assemble_item(source_item, tool_lookup, workspace_root)?;
                for attached_result in attached_results {
                    let result_item = timeline_items
                        .iter()
                        .find(|item| item.id().as_ref() == attached_result.item_id)?;
                    if let Some(result) =
                        Self::assemble_item(result_item, tool_lookup, workspace_root)
                    {
                        push_child_checked(&mut root, result, 1);
                        let embedded = tool_result_is_embedded(
                            tool_lookup,
                            match result_item {
                                OutputTimelineItem::ToolResult { reference } => {
                                    &reference.context.chat_id
                                }
                                _ => continue,
                            },
                            match result_item {
                                OutputTimelineItem::ToolResult { reference } => {
                                    &reference.context.run_id
                                }
                                _ => continue,
                            },
                            match result_item {
                                OutputTimelineItem::ToolResult { reference } => {
                                    &reference.tool_call_id
                                }
                                _ => continue,
                            },
                        );
                        if !embedded {
                            if let Some(standalone_result) =
                                Self::assemble_non_embedded_tool_result(result_item, tool_lookup)
                            {
                                return Some(standalone_result);
                            }
                        }
                    }
                }
                Some(root)
            }
            DisplayUnitPlan::ToolGroup {
                group_id,
                kind,
                member_ids,
                attached_results,
            } => {
                let mut group = leaf(
                    group_id.clone(),
                    OutputBlockKind::ToolGroup(ToolGroupBlockView {
                        key: group_id.clone(),
                        kind: *kind,
                        title: kind.title().to_string(),
                        style: SemanticStyle::Muted,
                    }),
                );
                for member_id in member_ids {
                    let source_item = timeline_items.iter().find(|item| {
                        matches!(item, OutputTimelineItem::ToolCall { .. })
                            && item.id().to_string().contains(member_id)
                    })?;
                    let member = Self::assemble_item(source_item, tool_lookup, workspace_root)?;
                    push_child_checked(&mut group, member, 1);
                }
                for attached_result in attached_results {
                    let result_item = timeline_items
                        .iter()
                        .find(|item| item.id().as_ref() == attached_result.item_id)?;
                    let Some(result) =
                        Self::assemble_item(result_item, tool_lookup, workspace_root)
                    else {
                        continue;
                    };
                    let parent = group.children.iter_mut().find(|child| match &child.kind {
                        OutputBlockKind::ToolCall(tool_call) => {
                            tool_call.tool_call_id.as_deref()
                                == Some(attached_result.call_id.as_str())
                        }
                        _ => false,
                    })?;
                    push_child_checked(parent, result, 2);
                }
                Some(group)
            }
        }
    }

    fn assemble_non_embedded_tool_result(
        item: &OutputTimelineItem,
        tool_lookup: &impl ToolCallLookup,
    ) -> Option<BlockNode> {
        let OutputTimelineItem::ToolResult { reference } = item else {
            return None;
        };
        let call = find_tool_call(
            tool_lookup,
            &reference.context.chat_id,
            &reference.context.run_id,
            &reference.tool_call_id,
        )?;
        let payload = call.result.as_ref()?;
        let display_output =
            display_text_for_tool_result(Some(&call.name), &payload.output, &payload.content);
        let mut text =
            summarize_non_embedded_result(Some(&call.name), &display_output, payload.is_error);
        if payload.image_count > 0 {
            text = format!("{text}\n[图片: {}]", payload.image_count);
        }
        let id = format!("{}-result", reference.tool_call_id.as_ref());
        Some(leaf(
            id.clone(),
            OutputBlockKind::DiagnosticNotice(TextBlockView {
                key: id,
                text,
                style: if payload.is_error {
                    SemanticStyle::Error
                } else {
                    SemanticStyle::Success
                },
            }),
        ))
    }

    pub(super) fn assemble_item(
        item: &OutputTimelineItem,
        tool_lookup: &impl ToolCallLookup,
        workspace_root: Option<&std::path::Path>,
    ) -> Option<BlockNode> {
        match item {
            OutputTimelineItem::UserMessage { id, text } => Some(leaf(
                id.clone(),
                OutputBlockKind::UserMessage(TextBlockView {
                    key: id.clone(),
                    text: text.clone(),
                    style: SemanticStyle::Normal,
                }),
            )),
            OutputTimelineItem::AssistantText { id, text, .. } => Some(leaf(
                id.clone(),
                OutputBlockKind::AssistantMessage(TextBlockView {
                    key: id.clone(),
                    text: text.clone(),
                    style: SemanticStyle::Normal,
                }),
            )),
            OutputTimelineItem::Thinking { id, text, .. } => Some(leaf(
                id.clone(),
                OutputBlockKind::ThinkingMessage(TextBlockView {
                    key: id.clone(),
                    text: text.clone(),
                    style: SemanticStyle::Muted,
                }),
            )),
            OutputTimelineItem::ToolCall { reference } => {
                let tool = find_tool_view(
                    tool_lookup,
                    &reference.context.chat_id,
                    &reference.context.run_id,
                    &reference.tool_call_id,
                    workspace_root,
                )?;
                let mut parent = leaf(tool.key.clone(), OutputBlockKind::ToolCall(tool.clone()));
                if let Some(result_text) = tool.result_summary.clone() {
                    let result_id = format!("{}-result", reference.tool_call_id.as_ref());
                    let child = leaf(
                        result_id.clone(),
                        OutputBlockKind::ToolResult(ToolResultBlockView {
                            key: result_id,
                            tool_title: tool.title.clone(),
                            args_preview: tool.args_preview.clone(),
                            result_text,
                            data: tool
                                .result_payload
                                .as_ref()
                                .map(|payload| payload.content.clone()),
                            style: tool.style,
                        }),
                    );
                    push_child_checked(&mut parent, child, 1);
                }
                Some(parent)
            }
            OutputTimelineItem::ToolResult { reference } => {
                if tool_result_is_embedded(
                    tool_lookup,
                    &reference.context.chat_id,
                    &reference.context.run_id,
                    &reference.tool_call_id,
                ) {
                    return None;
                }
                Self::assemble_non_embedded_tool_result(item, tool_lookup)
            }
            OutputTimelineItem::HookNotice {
                id,
                title,
                text,
                kind,
            } => Some(leaf(
                id.clone(),
                OutputBlockKind::HookNotice(HookNoticeBlockView {
                    key: id.clone(),
                    title: title.clone(),
                    body: text.clone(),
                    kind: kind.clone(),
                }),
            )),
            OutputTimelineItem::System { id, text } => Some(leaf(
                id.clone(),
                OutputBlockKind::SystemNotice(TextBlockView {
                    key: id.clone(),
                    text: text.clone(),
                    style: SemanticStyle::Muted,
                }),
            )),
            OutputTimelineItem::Error { id, text } => Some(leaf(
                id.clone(),
                OutputBlockKind::DiagnosticNotice(TextBlockView {
                    key: id.clone(),
                    text: text.clone(),
                    style: SemanticStyle::Error,
                }),
            )),
            OutputTimelineItem::QueuedUserMessage { .. } => None,
            OutputTimelineItem::AgentProgress { id, message, .. } => Some(leaf(
                id.clone(),
                OutputBlockKind::DiagnosticNotice(TextBlockView {
                    key: id.clone(),
                    text: message.clone(),
                    style: SemanticStyle::Running,
                }),
            )),
            OutputTimelineItem::AskUserBatch {
                id,
                slots,
                active_index,
                phase,
                cursor,
                selected,
                chat_input_active,
                chat_input_text,
                chat_input_cursor,
                confirm_cursor,
                completion,
                ..
            } => {
                use crate::tui::model::conversation::block::AskUserPhase as MPhase;
                let phase = match phase {
                    MPhase::Answering => AskUserPhaseView::Answering,
                    MPhase::Confirming => AskUserPhaseView::Confirming,
                };
                let slots = slots
                    .iter()
                    .map(|slot| AskUserSlotView {
                        id: slot.id.clone(),
                        question: slot.question.clone(),
                        options: slot.options.clone(),
                        llm_option_count: slot.llm_option_count,
                        multi_select: slot.multi_select,
                        default: slot.default.clone(),
                        answer: slot.answer.clone(),
                    })
                    .collect();
                Some(leaf(
                    id.clone(),
                    OutputBlockKind::AskUserBatch(AskUserBatchBlockView {
                        key: id.clone(),
                        slots,
                        active_index: *active_index,
                        phase,
                        cursor: *cursor,
                        selected: selected.clone(),
                        chat_input_active: *chat_input_active,
                        chat_input_text: chat_input_text.clone(),
                        chat_input_cursor: *chat_input_cursor,
                        confirm_cursor: *confirm_cursor,
                        completion: match completion {
                            crate::tui::model::conversation::block::AskUserCompletion::Active => {
                                crate::tui::view_model::output::AskUserCompletionView::Active
                            }
                            crate::tui::model::conversation::block::AskUserCompletion::ReplyPending => {
                                crate::tui::view_model::output::AskUserCompletionView::ReplyPending
                            }
                            crate::tui::model::conversation::block::AskUserCompletion::CancelPending => {
                                crate::tui::view_model::output::AskUserCompletionView::CancelPending
                            }
                            crate::tui::model::conversation::block::AskUserCompletion::Answered => {
                                crate::tui::view_model::output::AskUserCompletionView::Answered
                            }
                            crate::tui::model::conversation::block::AskUserCompletion::Cancelled => {
                                crate::tui::view_model::output::AskUserCompletionView::Cancelled
                            }
                        },
                    }),
                ))
            }
            OutputTimelineItem::OrphanToolResult {
                id,
                tool_name,
                output,
                content,
                is_error,
            } => {
                let display_output = display_text_for_tool_result(Some(tool_name), output, content);
                let text =
                    summarize_non_embedded_result(Some(tool_name), &display_output, *is_error);
                if text.is_empty() {
                    return None;
                }
                let id = format!("orphan-{id}");
                Some(leaf(
                    id.clone(),
                    OutputBlockKind::DiagnosticNotice(TextBlockView {
                        key: id,
                        text,
                        style: if *is_error {
                            SemanticStyle::Error
                        } else {
                            SemanticStyle::Success
                        },
                    }),
                ))
            }
        }
    }
}

/// 构造无子的叶子 BlockNode（block_version 取 kind 语义指纹）。
fn leaf(block_id: String, kind: OutputBlockKind) -> BlockNode {
    let block_version = kind.cache_version();
    BlockNode {
        block_id,
        block_version,
        kind,
        children: Vec::new(),
    }
}

/// 按嵌套规则表 + 深度上限校验后将 child 挂到 parent 下；不合法则记日志并丢弃（debug 断言失败）。
fn push_child_checked(parent: &mut BlockNode, child: BlockNode, depth: usize) {
    if !allowed_child(&parent.kind, &child.kind) || depth >= MAX_BLOCK_DEPTH {
        crate::tui::log_warn!(
            "drop illegal child block: parent={} child={} depth={depth}",
            parent.block_id,
            child.block_id
        );
        debug_assert!(false, "非法子块嵌套被丢弃，违反 nesting 规则");
        return;
    }
    parent.children.push(child);
}

#[cfg(test)]
fn assemble_output_window(
    conversation: &ConversationModel,
    workspace_root: Option<&std::path::Path>,
    window: crate::tui::view_model::OutputRenderWindow,
) -> OutputViewModel {
    super::retained_output_view::RetainedOutputView::default()
        .materialize_window(
            conversation,
            &crate::tui::model::display_history::DisplayHistoryModel::default(),
            workspace_root,
            window,
        )
        .view_model
}

#[cfg(test)]
fn assemble_output_view(
    conversation: &ConversationModel,
    workspace_root: Option<&std::path::Path>,
) -> OutputViewModel {
    assemble_output_window(
        conversation,
        workspace_root,
        crate::tui::view_model::OutputRenderWindow {
            line_limit: usize::MAX,
            tail_offset: 0,
        },
    )
}

#[cfg(test)]
#[path = "output_task_tests.rs"]
mod task_tests;
#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "output_unit_tests.rs"]
mod unit_tests;

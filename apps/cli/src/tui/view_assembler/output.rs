use crate::tui::model::conversation::block::HookNoticeKind;
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::ToolCall;
use crate::tui::model::output_timeline::OutputTimelineItem;
use crate::tui::view_model::{
    allowed_child, AskUserBatchBlockView, AskUserPhaseView, AskUserSlotView, BlockNode,
    HookNoticeBlockView, HookNoticeSemanticKind, OutputBlockKind, OutputViewModel, SemanticStyle,
    TextBlockView, ToolResultBlockView, MAX_BLOCK_DEPTH,
};
use std::collections::HashMap;

use super::output_tool_view::{
    display_text_for_tool_result, find_tool_call, find_tool_view, summarize_non_embedded_result,
    tool_result_is_embedded,
};

/// assemble 期一次性构建的工具查找索引，把 O(n²) 线性扫描降为 O(1)。
pub(super) struct ToolIndex<'a> {
    calls: HashMap<(&'a ChatId, &'a ChatTurnId, &'a ToolCallId), &'a ToolCall>,
}

impl<'a> ToolIndex<'a> {
    pub(super) fn build(conversation: &'a ConversationModel) -> Self {
        let mut calls = HashMap::new();
        for chat in &conversation.chats {
            for turn in &chat.turns {
                for call in &turn.tool_calls {
                    if let Some(id) = call.id.as_ref() {
                        calls.insert((&chat.id, &turn.id, id), call);
                    }
                }
            }
        }
        Self { calls }
    }

    pub(super) fn call(
        &self,
        chat_id: &ChatId,
        turn_id: &ChatTurnId,
        tool_id: &ToolCallId,
    ) -> Option<&'a ToolCall> {
        self.calls.get(&(chat_id, turn_id, tool_id)).copied()
    }
}

pub struct OutputViewAssembler;

impl OutputViewAssembler {
    pub fn assemble_from_conversation(
        conversation: &ConversationModel,
        version: u64,
        workspace_root: Option<&std::path::Path>,
    ) -> OutputViewModel {
        let mut roots: Vec<BlockNode> = Vec::new();
        let tool_index = ToolIndex::build(conversation);

        for item in conversation.timeline.items() {
            match item {
                OutputTimelineItem::UserMessage { id, text } => {
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::UserMessage(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Normal,
                        }),
                    ));
                }
                OutputTimelineItem::AssistantText { id, text, .. } => {
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::AssistantMessage(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Normal,
                        }),
                    ));
                }
                OutputTimelineItem::Thinking { id, text, .. } => {
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::ThinkingMessage(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Muted,
                        }),
                    ));
                }
                OutputTimelineItem::ToolCall { reference } => {
                    if let Some(tool) = find_tool_view(
                        &tool_index,
                        &reference.context.chat_id,
                        &reference.context.turn_id,
                        &reference.tool_call_id,
                        workspace_root,
                    ) {
                        let mut parent =
                            leaf(tool.key.clone(), OutputBlockKind::ToolCall(tool.clone()));
                        // 工具结果升为子块：取 result_summary 同源文本，附加为 depth-1 子节点。
                        if let Some(result_text) = tool.result_summary.clone() {
                            let result_id = format!("{}-result", reference.tool_call_id.as_ref());
                            let child = leaf(
                                result_id.clone(),
                                OutputBlockKind::ToolResult(ToolResultBlockView {
                                    key: result_id,
                                    tool_title: tool.title.clone(),
                                    args_preview: tool.args_preview.clone(),
                                    result_text,
                                    data: tool.result_payload.as_ref().map(|p| p.content.clone()),
                                    style: tool.style,
                                }),
                            );
                            push_child_checked(&mut parent, child, 1);
                        }
                        roots.push(parent);
                    }
                }
                OutputTimelineItem::ToolResult { reference } => {
                    if tool_result_is_embedded(
                        &tool_index,
                        &reference.context.chat_id,
                        &reference.context.turn_id,
                        &reference.tool_call_id,
                    ) {
                        continue;
                    }
                    // A4.5: 从 chats.tool_calls[].result（ToolResultPayload）读，不再读 blocks。
                    let Some(call) = find_tool_call(
                        &tool_index,
                        &reference.context.chat_id,
                        &reference.context.turn_id,
                        &reference.tool_call_id,
                    ) else {
                        continue;
                    };
                    let Some(payload) = call.result.as_ref() else {
                        continue;
                    };
                    let tool_name = Some(call.name.as_str());
                    let display_output =
                        display_text_for_tool_result(tool_name, &payload.output, &payload.content);
                    let text =
                        summarize_non_embedded_result(tool_name, &display_output, payload.is_error);
                    let text = if payload.image_count > 0 {
                        format!("{text}\n[图片: {}]", payload.image_count)
                    } else {
                        text
                    };
                    roots.push(leaf(
                        format!("{}-result", reference.tool_call_id.as_ref()),
                        OutputBlockKind::DiagnosticNotice(TextBlockView {
                            key: format!("{}-result", reference.tool_call_id.as_ref()),
                            text,
                            style: if payload.is_error {
                                SemanticStyle::Error
                            } else {
                                SemanticStyle::Success
                            },
                        }),
                    ));
                }
                OutputTimelineItem::System { id, text } => {
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::SystemNotice(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Muted,
                        }),
                    ));
                }
                OutputTimelineItem::HookNotice { id, content } => {
                    let (kind, style) = match content.kind {
                        HookNoticeKind::Blocked => {
                            (HookNoticeSemanticKind::Blocked, SemanticStyle::Error)
                        }
                        HookNoticeKind::Failed => {
                            (HookNoticeSemanticKind::Failed, SemanticStyle::Error)
                        }
                        HookNoticeKind::Info => {
                            (HookNoticeSemanticKind::Info, SemanticStyle::Muted)
                        }
                    };
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::HookNotice(HookNoticeBlockView {
                            key: id.clone(),
                            kind,
                            title: content.title.clone(),
                            body: content.body.clone(),
                            details: content.details.clone(),
                            style,
                        }),
                    ));
                }
                OutputTimelineItem::Error { id, text } => {
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::DiagnosticNotice(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Error,
                        }),
                    ));
                }
                OutputTimelineItem::QueuedUserMessage { .. } => {
                    // 排队输入不再作为 document block 渲染，改为在 spinner 上方固定显示。
                }
                OutputTimelineItem::AgentProgress { id, message, .. } => {
                    // 当前无 mutation 推此 timeline 项；agent 进度内联于 tool_calls[].activities（activity_lines）。误接入会重现 A4.2 双显示回归。
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::DiagnosticNotice(TextBlockView {
                            key: id.clone(),
                            text: message.clone(),
                            style: SemanticStyle::Running,
                        }),
                    ));
                }
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
                    confirmed,
                } => {
                    use crate::tui::model::conversation::block::AskUserPhase as MPhase;
                    let phase_view = match phase {
                        MPhase::Answering => AskUserPhaseView::Answering,
                        MPhase::Confirming => AskUserPhaseView::Confirming,
                    };
                    let slots_view: Vec<_> = slots
                        .iter()
                        .map(|s| AskUserSlotView {
                            id: s.id.clone(),
                            question: s.question.clone(),
                            options: s.options.clone(),
                            llm_option_count: s.llm_option_count,
                            multi_select: s.multi_select,
                            default: s.default.clone(),
                            answer: s.answer.clone(),
                        })
                        .collect();
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::AskUserBatch(AskUserBatchBlockView {
                            key: id.clone(),
                            slots: slots_view,
                            active_index: *active_index,
                            phase: phase_view,
                            cursor: *cursor,
                            selected: selected.clone(),
                            chat_input_active: *chat_input_active,
                            chat_input_text: chat_input_text.clone(),
                            chat_input_cursor: *chat_input_cursor,
                            confirm_cursor: *confirm_cursor,
                            confirmed: *confirmed,
                        }),
                    ));
                }
                OutputTimelineItem::OrphanToolResult {
                    id,
                    tool_name,
                    output,
                    content,
                    is_error,
                } => {
                    // 与非嵌入路径一致（DRY）：只展示工具摘要（如 `✓ Read completed`），
                    // 绝不把完整原始 output 当正文逐行刷出；颜色随成功/失败而非 Warning（#87）。
                    let display_output =
                        display_text_for_tool_result(Some(tool_name), output, content);
                    let text =
                        summarize_non_embedded_result(Some(tool_name), &display_output, *is_error);
                    if text.is_empty() {
                        continue;
                    }
                    roots.push(leaf(
                        format!("orphan-{id}"),
                        OutputBlockKind::DiagnosticNotice(TextBlockView {
                            key: format!("orphan-{id}"),
                            text,
                            style: if *is_error {
                                SemanticStyle::Error
                            } else {
                                SemanticStyle::Success
                            },
                        }),
                    ));
                }
            }
        }
        OutputViewModel {
            roots,
            version,
            follow_tail_hint: true,
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
#[path = "output_task_tests.rs"]
mod task_tests;
#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "output_unit_tests.rs"]
mod unit_tests;

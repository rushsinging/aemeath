use crate::tui::model::conversation::block::HookNoticeKind;
use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::{ToolCall, ToolCallStatus};
use crate::tui::model::output_timeline::OutputTimelineItem;
use crate::tui::view_model::tool_name::tool_display_name;
use crate::tui::view_model::{
    allowed_child, AskUserBatchBlockView, AskUserPhaseView, AskUserSlotView, BlockNode,
    HookNoticeBlockView, HookNoticeSemanticKind, OutputBlockKind, OutputViewModel, SemanticStyle,
    TextBlockView, ToolCallBlockView, ToolResultBlockView, ToolSemanticStatus, MAX_BLOCK_DEPTH,
};

pub struct OutputViewAssembler;

impl OutputViewAssembler {
    pub fn assemble_from_conversation(
        conversation: &ConversationModel,
        version: u64,
    ) -> OutputViewModel {
        let mut roots: Vec<BlockNode> = Vec::new();
        if conversation.timeline.items().is_empty() {
            assemble_legacy_conversation_blocks(conversation, &mut roots);
        }

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
                        conversation,
                        &reference.context.chat_id,
                        &reference.context.turn_id,
                        &reference.tool_call_id,
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
                        conversation,
                        &reference.context.chat_id,
                        &reference.context.turn_id,
                        &reference.tool_call_id,
                    ) {
                        continue;
                    }
                    let tool_name = find_tool_name_by_id(
                        conversation,
                        &reference.context.chat_id,
                        &reference.context.turn_id,
                        &reference.tool_call_id,
                    );
                    let Some(result) = find_tool_result_payload(
                        conversation,
                        &reference.context.chat_id,
                        &reference.context.turn_id,
                        &reference.tool_call_id,
                    ) else {
                        continue;
                    };
                    let display_output = display_text_for_tool_result(
                        tool_name.as_deref(),
                        result.output,
                        result.content,
                    );
                    let text = summarize_non_embedded_result(
                        tool_name.as_deref(),
                        &display_output,
                        result.is_error,
                    );
                    let text = if result.image_count > 0 {
                        format!("{text}\n[图片: {}]", result.image_count)
                    } else {
                        text
                    };
                    roots.push(leaf(
                        format!("{}-result", reference.tool_call_id.as_ref()),
                        OutputBlockKind::DiagnosticNotice(TextBlockView {
                            key: format!("{}-result", reference.tool_call_id.as_ref()),
                            text,
                            style: if result.is_error {
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

fn tool_result_is_embedded(
    conversation: &ConversationModel,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_id: &ToolCallId,
) -> bool {
    find_tool_call(conversation, chat_id, turn_id, tool_id)
        .and_then(|call| call.result.as_ref())
        .is_some_and(|result| !result.is_empty())
}

/// 查找指定 runtime context 下 tool call id 对应的工具名称。
fn find_tool_name_by_id(
    conversation: &ConversationModel,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_id: &ToolCallId,
) -> Option<String> {
    find_tool_call(conversation, chat_id, turn_id, tool_id).map(|call| call.name.clone())
}

/// 对非嵌入/孤儿 ToolResult 生成摘要文本：优先用工具的 `format_result_summary`，
/// 工具名未知（id 错位导致 `find_tool_name_by_id`=None）时回退通用完成摘要。
///
/// **绝不**把完整原始 output 当文本刷出——这是 #87 的泄漏源（旧逻辑在工具名未知时
/// 截断原始 output 当摘要，导致带行号正文 + "lines omitted" 刷屏）。
fn summarize_non_embedded_result(tool_name: Option<&str>, output: &str, is_error: bool) -> String {
    if output.is_empty() {
        return String::new();
    }
    // 无工具名时用占位名走通用完成摘要（如 `✓ Tool completed`），仍不泄漏正文。
    let name = tool_name.map(tool_display_name).unwrap_or("Tool");
    default_tool_result_summary(name, is_error).join("\n")
}

fn find_tool_view(
    conversation: &ConversationModel,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_id: &ToolCallId,
) -> Option<ToolCallBlockView> {
    let call = find_tool_call(conversation, chat_id, turn_id, tool_id)?;
    let (icon, semantic_status, style) = map_tool_status(call.status);
    let result_summary = call
        .result
        .as_deref()
        .filter(|result| !result.is_empty())
        .map(|result| {
            find_tool_result_payload(conversation, chat_id, turn_id, tool_id)
                .map(|payload| {
                    display_text_for_tool_result(Some(&call.name), result, payload.content)
                })
                .unwrap_or_else(|| result.to_string())
        });
    crate::tui::log_debug!(
        "assemble tool_call_view chat_id={} turn_id={} id={} name={} status={:?} args_len={} result_len={} activity_count={}",
        chat_id.as_ref(),
        turn_id.as_ref(),
        tool_id.as_ref(),
        call.name,
        call.status,
        call.args_preview.len(),
                result_summary.as_ref().map(|value| value.len()).unwrap_or(0),
        call.activities.len(),
    );
    Some(ToolCallBlockView {
        key: format!(
            "{}/{}/{}",
            chat_id.as_ref(),
            turn_id.as_ref(),
            tool_id.as_ref()
        ),
        chat_id: Some(chat_id.as_ref().to_string()),
        turn_id: Some(turn_id.as_ref().to_string()),
        tool_call_id: Some(tool_id.as_ref().to_string()),
        title: call.name.clone(),
        icon: icon.to_string(),
        semantic_status,
        style,
        args_preview: (!call.args_preview.is_empty()).then(|| call.args_preview.clone()),
        // 工具已完成时不再显示 activity_summary（结果已在 ToolResult 子块展示，
        // 避免子代理最终输出同时出现在 activity 行和 result 子块中造成重复）。
        activity_summary: if matches!(
            call.status,
            ToolCallStatus::Success | ToolCallStatus::Error | ToolCallStatus::Cancelled
        ) {
            None
        } else {
            call.activities.last().cloned()
        },
        // result 子块展示实际工具 output（供渲染层 format_result_lines 按
        // result_max_lines 截断成前 N 行预览）；完整内容不刷屏由渲染层截断 + id
        // 不丢（bind 修复）共同保证，不再退化为纯 "✓ X completed" 摘要。
        result_summary,
        collapsible: true,
        collapsed: false,
    })
}

struct ToolResultPayload<'a> {
    output: &'a str,
    content: &'a serde_json::Value,
    is_error: bool,
    image_count: usize,
}

fn find_tool_call<'a>(
    conversation: &'a ConversationModel,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_id: &ToolCallId,
) -> Option<&'a ToolCall> {
    conversation
        .chats
        .iter()
        .find(|chat| &chat.id == chat_id)
        .and_then(|chat| chat.turns.iter().find(|turn| &turn.id == turn_id))
        .and_then(|turn| {
            turn.tool_calls
                .iter()
                .find(|call| call.id.as_ref() == Some(tool_id))
        })
}

fn find_tool_result_payload<'a>(
    conversation: &'a ConversationModel,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_id: &ToolCallId,
) -> Option<ToolResultPayload<'a>> {
    conversation.blocks.iter().find_map(|block| match block {
        crate::tui::model::conversation::block::ConversationBlock::ToolResult {
            id,
            chat_id: result_chat_id,
            turn_id: result_turn_id,
            output,
            content,
            is_error,
            image_count,
        } if id == tool_id && result_chat_id == chat_id && result_turn_id == turn_id => {
            Some(ToolResultPayload {
                output,
                content,
                is_error: *is_error,
                image_count: *image_count,
            })
        }
        _ => None,
    })
}

fn assemble_legacy_conversation_blocks(
    conversation: &ConversationModel,
    roots: &mut Vec<BlockNode>,
) {
    for block in &conversation.blocks {
        let crate::tui::model::conversation::block::ConversationBlock::ToolResult {
            id,
            chat_id,
            turn_id,
            output,
            content,
            is_error,
            image_count,
        } = block
        else {
            continue;
        };
        if tool_result_is_embedded(conversation, chat_id, turn_id, id) {
            continue;
        }
        let tool_name = find_tool_name_by_id(conversation, chat_id, turn_id, id);
        let display_output = display_text_for_tool_result(tool_name.as_deref(), output, content);
        let text = summarize_non_embedded_result(tool_name.as_deref(), &display_output, *is_error);
        let text = if *image_count > 0 {
            format!("{text}\n[图片: {image_count}]")
        } else {
            text
        };
        roots.push(leaf(
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
}

fn display_text_for_tool_result(
    tool_name: Option<&str>,
    fallback_output: &str,
    content: &serde_json::Value,
) -> String {
    // 建议同步（#196）：Bash/Read 工具结果在进入 TUI 渲染前把 `\t` 展开为 4 空格，
    // 避免 Buffer::set_stringn 把 `\t` 当控制字符过滤带来的列宽不一致。
    // 不用 `sanitize_for_display` 是因为它会同时剥掉 `\n`，破坏多行 Read 输出。
    if matches!(tool_name, Some("EnterWorktree" | "ExitWorktree")) {
        let message = content
            .get("message")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty());
        let branch = content
            .get("branch")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty());
        match (message, branch) {
            (Some(message), Some(branch)) => {
                return expand_tabs(&format!("{message}\n当前分支：{branch}"));
            }
            (Some(message), None) => return expand_tabs(message).to_string(),
            _ => {}
        }
    }
    let text = content
        .get("display")
        .and_then(|value| value.as_str())
        .or_else(|| content.get("message").and_then(|value| value.as_str()))
        .or_else(|| content.get("text").and_then(|value| value.as_str()))
        .map(str::to_string)
        .unwrap_or_else(|| fallback_output.to_string());
    expand_tabs(&text).to_string()
}

/// 把 `\t` 展开为 4 空格（issue #196 建议同步专用）。其它控制字符与换行一律保留，
/// 不调用 `sanitize_for_display` 以免破坏多行 tool result。
fn expand_tabs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == '\t' {
            out.push_str("    ");
        } else {
            out.push(ch);
        }
    }
    out
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
#[path = "output_task_tests.rs"]
mod task_tests;
#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "output_unit_tests.rs"]
mod unit_tests;

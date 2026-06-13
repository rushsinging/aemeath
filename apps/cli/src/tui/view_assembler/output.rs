use crate::tui::model::conversation::block::ConversationBlock;
use crate::tui::model::conversation::ids::ToolCallId;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::view_model::{
    allowed_child, AskUserBlockView, BlockNode, HookNoticeBlockView, HookNoticeSemanticKind,
    OutputBlockKind, OutputViewModel, SemanticStyle, TextBlockView, ToolCallBlockView,
    ToolResultBlockView, ToolSemanticStatus, MAX_BLOCK_DEPTH,
};

pub struct OutputViewAssembler;

impl OutputViewAssembler {
    pub fn assemble_from_conversation(
        conversation: &ConversationModel,
        version: u64,
    ) -> OutputViewModel {
        let mut roots: Vec<BlockNode> = Vec::new();
        for conversation_block in &conversation.blocks {
            match conversation_block {
                ConversationBlock::UserMessage { id, text } => {
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::UserMessage(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Normal,
                        }),
                    ));
                }
                ConversationBlock::AssistantText { id, text, .. } => {
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::AssistantMessage(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Normal,
                        }),
                    ));
                }
                ConversationBlock::Thinking { id, text, .. } => {
                    roots.push(leaf(
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
                        let mut parent =
                            leaf(tool.key.clone(), OutputBlockKind::ToolCall(tool.clone()));
                        // 工具结果升为子块：取 result_summary 同源文本，附加为 depth-1 子节点。
                        if let Some(result_text) = tool.result_summary.clone() {
                            let result_id = format!("{}-result", id.as_ref());
                            let child = leaf(
                                result_id.clone(),
                                OutputBlockKind::ToolResult(ToolResultBlockView {
                                    key: result_id,
                                    tool_title: tool.title.clone(),
                                    summary: tool.summary.clone(),
                                    result_text,
                                    style: tool.style,
                                }),
                            );
                            push_child_checked(&mut parent, child, 1);
                        }
                        roots.push(parent);
                    }
                }
                ConversationBlock::ToolResult {
                    id,
                    output,
                    content,
                    is_error,
                    image_count,
                    ..
                } => {
                    if tool_result_is_embedded(conversation, id) {
                        continue;
                    }
                    let tool_name = find_tool_name_by_id(conversation, id);
                    let display_output =
                        display_text_for_tool_result(tool_name.as_deref(), output, content);
                    let text = summarize_non_embedded_result(
                        tool_name.as_deref(),
                        &display_output,
                        *is_error,
                    );
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
                ConversationBlock::System { id, text } => {
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::SystemNotice(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Muted,
                        }),
                    ));
                }
                ConversationBlock::HookNotice { id, content } => {
                    let (kind, style) = match content.kind {
                        crate::tui::model::conversation::block::HookNoticeKind::Blocked => {
                            (HookNoticeSemanticKind::Blocked, SemanticStyle::Warning)
                        }
                        crate::tui::model::conversation::block::HookNoticeKind::Failed => {
                            (HookNoticeSemanticKind::Failed, SemanticStyle::Error)
                        }
                        crate::tui::model::conversation::block::HookNoticeKind::Info => {
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
                ConversationBlock::Error { id, text } => {
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::DiagnosticNotice(TextBlockView {
                            key: id.clone(),
                            text: text.clone(),
                            style: SemanticStyle::Error,
                        }),
                    ));
                }
                ConversationBlock::QueuedUserMessage { .. } => {
                    // 排队输入不再作为 document block 渲染，改为在 spinner 上方固定显示。
                }
                ConversationBlock::AgentProgress {
                    id,
                    tool_id: _,
                    message,
                } => {
                    roots.push(leaf(
                        id.clone(),
                        OutputBlockKind::DiagnosticNotice(TextBlockView {
                            key: id.clone(),
                            text: message.clone(),
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
                    chat_input_text,
                    default,
                    answer,
                } => {
                    roots.push(leaf(
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
                            chat_input_text: chat_input_text.clone(),
                            default: default.clone(),
                            answer: answer.clone(),
                        }),
                    ));
                }
                ConversationBlock::OrphanToolResult {
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

/// 查找 tool call id 对应的工具名称（遍历 conversation 中的所有 tool_calls）。
fn find_tool_name_by_id(conversation: &ConversationModel, tool_id: &ToolCallId) -> Option<String> {
    for chat in &conversation.chats {
        for turn in &chat.turns {
            for call in &turn.tool_calls {
                if call.id.as_ref() == Some(tool_id) {
                    return Some(call.name.clone());
                }
            }
        }
    }
    None
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
    let name = tool_name.unwrap_or("Tool");
    default_tool_result_summary(name, is_error).join("\n")
}

fn find_tool_view(conversation: &ConversationModel, tool_id: &str) -> Option<ToolCallBlockView> {
    for chat in &conversation.chats {
        for turn in &chat.turns {
            for call in &turn.tool_calls {
                if call.id.as_ref().map(|id| id.as_ref()) != Some(tool_id) {
                    continue;
                }
                let (icon, semantic_status, style) = map_tool_status(call.status);
                let result_summary = call
                    .result
                    .as_deref()
                    .filter(|result| !result.is_empty())
                    .map(|result| {
                        find_tool_result_content(conversation, tool_id)
                            .map(|content| {
                                display_text_for_tool_result(Some(&call.name), result, content)
                            })
                            .unwrap_or_else(|| result.to_string())
                    });
                log::debug!(
                    target: "cli::tui::tool_flow",                    "assemble tool_call_view chat_id={} turn_id={} id={} name={} status={:?} args_len={} summary_len={} result_len={} activity_count={}",
                    chat.id.as_ref(),
                    turn.id.as_ref(),
                    tool_id,
                    call.name,
                    call.status,
                    call.args_preview.len(),
                    call.summary.as_ref().map(|value| value.len()).unwrap_or(0),
                    result_summary.as_ref().map(|value| value.len()).unwrap_or(0),
                    call.activities.len(),
                );
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
                    // result 子块展示实际工具 output（供渲染层 format_result_lines 按
                    // result_max_lines 截断成前 N 行预览）；完整内容不刷屏由渲染层截断 + id
                    // 不丢（bind 修复）共同保证，不再退化为纯 "✓ X completed" 摘要。
                    result_summary,
                    collapsible: true,
                    collapsed: false,
                });
            }
        }
    }
    None
}

fn find_tool_result_content<'a>(
    conversation: &'a ConversationModel,
    tool_id: &str,
) -> Option<&'a serde_json::Value> {
    conversation.blocks.iter().find_map(|block| match block {
        ConversationBlock::ToolResult { id, content, .. } if id.as_ref() == tool_id => {
            Some(content)
        }
        ConversationBlock::OrphanToolResult { id, content, .. } if id == tool_id => Some(content),
        _ => None,
    })
}

fn display_text_for_tool_result(
    tool_name: Option<&str>,
    fallback_output: &str,
    content: &serde_json::Value,
) -> String {
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
                return format!("{message}\n当前分支：{branch}");
            }
            (Some(message), None) => return message.to_string(),
            _ => {}
        }
    }
    content
        .get("display")
        .and_then(|value| value.as_str())
        .or_else(|| content.get("message").and_then(|value| value.as_str()))
        .or_else(|| content.get("text").and_then(|value| value.as_str()))
        .map(str::to_string)
        .unwrap_or_else(|| fallback_output.to_string())
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

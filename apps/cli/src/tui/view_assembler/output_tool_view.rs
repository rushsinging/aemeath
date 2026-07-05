use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
use crate::tui::model::conversation::tool_call::{ToolCall, ToolCallStatus};
use crate::tui::view_model::conversation::tool_result_payload::ToolResultPayload;
use crate::tui::view_model::tool_name::tool_display_name;
use crate::tui::view_model::{AgentMetaView, SemanticStyle, ToolCallBlockView, ToolSemanticStatus};

use crate::tui::view_assembler::output::ToolIndex;

pub(super) fn tool_result_is_embedded(
    index: &ToolIndex<'_>,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_id: &ToolCallId,
) -> bool {
    find_tool_call(index, chat_id, turn_id, tool_id)
        .and_then(|call| call.result.as_ref())
        .is_some_and(|payload| !payload.output.is_empty())
}

/// 对非嵌入/孤儿 ToolResult 生成摘要文本：优先用工具的 `format_result_summary`，
/// 工具名未知时回退通用完成摘要。
///
/// **绝不**把完整原始 output 当文本刷出——这是 #87 的泄漏源（旧逻辑在工具名未知时
/// 截断原始 output 当摘要，导致带行号正文 + "lines omitted" 刷屏）。
pub(super) fn summarize_non_embedded_result(
    tool_name: Option<&str>,
    output: &str,
    is_error: bool,
) -> String {
    if output.is_empty() {
        return String::new();
    }
    // 无工具名时用占位名走通用完成摘要（如 `✓ Tool completed`），仍不泄漏正文。
    let name = tool_name.map(tool_display_name).unwrap_or("Tool");
    default_tool_result_summary(name, is_error).join("\n")
}

pub(super) fn find_tool_view(
    index: &ToolIndex<'_>,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_id: &ToolCallId,
    workspace_root: Option<&std::path::Path>,
) -> Option<ToolCallBlockView> {
    let call = find_tool_call(index, chat_id, turn_id, tool_id)?;
    let (icon, semantic_status, style) = map_tool_status(call.status);
    // 同时计算 result_summary（展示文本）与 result_payload（结构化 payload，
    // 供 TUI Display 走 typed 字段渲染 header）。
    // A4.1/A4.5: 直接从 ChatTurn.tool_calls[i].result（ToolResultPayload）取字段，
    // 不读 blocks（blocks fallback 已在 A4.5 删除）。
    let (result_summary, result_payload) = match call
        .result
        .as_ref()
        .filter(|payload| !payload.output.is_empty())
    {
        Some(model_payload) => {
            let view_payload = ToolResultPayload::new(
                model_payload.output.clone(),
                model_payload.content.clone(),
                model_payload.is_error,
                model_payload.image_count,
            );
            let text = display_text_for_tool_result(
                Some(&call.name),
                &model_payload.output,
                &model_payload.content,
            );
            (Some(text), Some(view_payload))
        }
        None => (None, None),
    };
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
        result_payload,
        workspace_root: workspace_root.map(|p| p.to_path_buf()),
        collapsible: true,
        collapsed: false,
        agent_meta: call.agent_meta.as_ref().map(|m| AgentMetaView {
            role: m.role.clone(),
            model: m.model.clone(),
        }),
    })
}

pub(super) fn find_tool_call<'a>(
    index: &ToolIndex<'a>,
    chat_id: &ChatId,
    turn_id: &ChatTurnId,
    tool_id: &ToolCallId,
) -> Option<&'a ToolCall> {
    index.call(chat_id, turn_id, tool_id)
}

pub(super) fn display_text_for_tool_result(
    tool_name: Option<&str>,
    fallback_output: &str,
    content: &serde_json::Value,
) -> String {
    // 建议同步（#196）：Bash/Read 工具结果在进入 TUI 渲染前把 `\t` 展开为 4 空格，
    // 避免底层 buffer 写入把 `\t` 当控制字符过滤带来的列宽不一致。
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
    // Edit 工具的 diff 内容已通过结构化 data 通道（ToolResultBlockView.data）
    // 直接传给渲染层（edit_diff::edit_diff_from_data），无需在 display_text 里拼装。
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

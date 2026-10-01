use crate::tui::render::output::primitives::wrap::{wrap_spans_with_prefix, WrapMode};
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::output::tool_display::format_tool_call;
use crate::tui::render::theme;
use crate::tui::view_model::output::ToolCallBlockView;
use crate::tui::view_model::AgentMetaView;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::rc::Rc;

/// 渲染工具调用块：仅 header（标题）+ args detail 行。
///
/// streaming preview 与最终结果均升为独立 ToolResult 子块（#1547），
/// 由 assembler 作为 depth-1 子节点附加，marker/缩进/续行由 gutter 统一管理。
/// 此处不再渲染任何 activity 内联行。
pub fn render_tool_call(
    block_id: &str,
    view: &ToolCallBlockView,
    ctx: &RenderCtx,
) -> RenderedBlock {
    let header_input = view.args_preview.as_deref().filter(|s| !s.is_empty());
    let (header_line, detail_lines) = header_input
        .map(|raw_json| {
            // issue #499：Agent 工具的 role/model 可能由 runtime resolve（如 user 指定
            // role 但未指定 model）。此时 input JSON 不含实际 model，而 agent_meta 携带
            // resolve 后的值。将 agent_meta 合并到 JSON 副本，format_tool_call 自然取到。
            let effective_json = merge_agent_meta(raw_json, view.agent_meta.as_ref());
            format_tool_call(
                &view.title,
                &effective_json,
                view.result_payload.as_ref(),
                view.workspace_root.as_deref(),
            )
        })
        .unwrap_or_else(|| {
            // issue #839：args_preview 为 None（PendingArgs 阶段）时也走 format_tool_call，
            // 传入空 JSON "{}" 使 format_header 生成 display_name fallback，
            // 与正常路径行为一致，避免退化为裸 display name。
            let effective_json = merge_agent_meta("{}", view.agent_meta.as_ref());
            format_tool_call(
                &view.title,
                &effective_json,
                view.result_payload.as_ref(),
                view.workspace_root.as_deref(),
            )
        });
    crate::tui::log_debug!(
        "render tool_call block_id={} title={} status={:?} args_len={} result_len={} detail_lines={} streaming_preview={}",
        block_id,
        view.title,
        view.semantic_status,
        view.args_preview.as_ref().map(|value| value.len()).unwrap_or(0),
        view.result_summary.as_ref().map(|value| value.len()).unwrap_or(0),
        detail_lines.len(),
        // streaming preview 已升为独立 ToolResult 子块，此处只记录是否存在预览。
        view.streaming_preview.is_some(),
    );
    // header / detail 两部分消费 ctx.text_width 做 wrap（Word 模式），避免窄终端下行宽
    // 超出 output_document_width 被 ratatui 截断。marker 由 gutter 注入，header 只渲染
    // 去掉前导 ● 的标题文本。
    let header_style = Style::default().fg(theme::TEXT);
    let detail_style = Style::default().fg(theme::TEXT_MUTED);
    let width = ctx.text_width as usize;

    let header_line = strip_leading_bullet(header_line);
    let mut lines: Vec<RenderedLine> =
        wrap_spans_with_prefix(header_line.spans, width, None, WrapMode::Word)
            .into_iter()
            .map(|line| line.with_style(header_style))
            .collect();

    for detail in detail_lines {
        lines.extend(
            wrap_spans_with_prefix(
                vec![Span::styled(detail, detail_style)],
                width,
                None,
                WrapMode::Word,
            )
            .into_iter()
            .map(|line| line.with_style(detail_style)),
        );
    }
    // streaming preview 已升为独立 ToolResult 子块；ToolCall 只渲染 header/detail。
    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(lines),
    }
}

/// 从 Line 的文本内容中去掉前导 `●` marker 并 trim 空白。
/// 操作方式：如果第一个 span 以 `●` 开头，移除该前缀并 trim_start。
fn strip_leading_bullet(mut line: Line<'static>) -> Line<'static> {
    if let Some(first) = line.spans.first_mut() {
        let content: &str = first.content.as_ref();
        if let Some(stripped) = content.strip_prefix('●') {
            first.content = std::borrow::Cow::Owned(stripped.trim_start().to_string());
        }
    }
    line
}

/// 将 agent_meta 的 role/model 合并到 Agent tool 的 input JSON 字符串。
///
/// issue #499：当 user 只指定 role（未指定 model）时，runtime 根据 role 配置
/// resolve 出实际 model。agent_meta 携带 resolve 后的值，此处合并到 JSON 副本，
/// 使 `AgentDisplay::format_header` 能取到实际 model。
fn merge_agent_meta(raw_json: &str, meta: Option<&AgentMetaView>) -> String {
    use serde_json::Value;
    let Some(meta) = meta else {
        return raw_json.to_string();
    };
    let mut json: Value =
        serde_json::from_str(raw_json).unwrap_or(Value::Object(Default::default()));
    if let Value::Object(ref mut obj) = json {
        if let Some(role) = &meta.role {
            obj.insert("role".to_string(), Value::String(role.clone()));
        }
        if !meta.model.is_empty() {
            obj.insert("model".to_string(), Value::String(meta.model.clone()));
        }
    }
    serde_json::to_string(&json).unwrap_or_else(|_| raw_json.to_string())
}

#[cfg(test)]
#[path = "tool_call_tests.rs"]
mod tests;

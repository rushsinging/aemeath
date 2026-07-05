use crate::tui::render::output::primitives::wrap::{wrap_spans_with_prefix, WrapMode};
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::output::tool_display::format_tool_call;
use crate::tui::render::theme;
use crate::tui::view_model::output::ToolCallBlockView;
use crate::tui::view_model::tool_name::tool_display_name;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use std::rc::Rc;
use unicode_width::UnicodeWidthStr;

/// 渲染工具调用块：仅 header（标题）+ args detail 行 + 可选的 activity 状态行。
///
/// 工具结果已升为独立子块（`ToolResult` 变体，见 `blocks/tool_result.rs`，#60），
/// 由 assembler 作为本块的 depth-1 子节点附加，此处不再渲染结果。
pub fn render_tool_call(
    block_id: &str,
    view: &ToolCallBlockView,
    ctx: &RenderCtx,
) -> RenderedBlock {
    let header_input = view.args_preview.as_deref().filter(|s| !s.is_empty());
    let (header_line, detail_lines) = header_input
        .map(|raw_json| {
            format_tool_call(
                &view.title,
                raw_json,
                view.result_payload.as_ref(),
                view.workspace_root.as_deref(),
            )
        })
        .unwrap_or_else(|| {
            (
                Line::from(vec![
                    Span::raw("● "),
                    Span::styled(
                        tool_display_name(&view.title).to_string(),
                        Style::default().fg(theme::ACCENT_BRIGHT),
                    ),
                ]),
                Vec::new(),
            )
        });
    crate::tui::log_debug!(
        "render tool_call block_id={} title={} status={:?} args_len={}  result_len={} detail_lines={} activity_present={}",
        block_id,
        view.title,
        view.semantic_status,
        view.args_preview.as_ref().map(|value| value.len()).unwrap_or(0),
                view.result_summary.as_ref().map(|value| value.len()).unwrap_or(0),
        detail_lines.len(),
        view.activity_summary.is_some(),
    );
    // issue #361：header / detail / activity 三部分均消费 ctx.text_width 做 wrap（Word
    // 模式），避免窄终端下行宽超出 output_document_width 被 ratatui 截断。marker 由 gutter
    // 注入，header 只渲染去掉前导 ● 的标题文本。line base style（TEXT / TEXT_MUTED）让未
    // 显式着色的 span 继承主题色，已有显式颜色的 span（如 display_name 的 ACCENT_BRIGHT）保留。
    let header_style = Style::default().fg(theme::TEXT);
    let detail_style = Style::default().fg(theme::TEXT_MUTED);
    let width = ctx.text_width as usize;

    let header_line = strip_leading_bullet(header_line);
    // issue #499：在 header 前注入 [role] provider/model 元信息 span，按可用宽度截断。
    let meta_spans = build_meta_spans(view, width);
    let header_line = if meta_spans.is_empty() {
        header_line
    } else {
        prepend_spans(header_line, meta_spans)
    };
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
    // 渲染 activity_summary：Agent 等长时间工具执行过程中显示当前进度（如子 agent 当前操作），
    // 嵌套在 ToolCall block 内而非根级 DiagnosticNotice 泄露到对话流中。
    if let Some(activity) = &view.activity_summary {
        lines.extend(
            wrap_spans_with_prefix(
                vec![Span::styled(activity.clone(), detail_style)],
                width,
                None,
                WrapMode::Word,
            )
            .into_iter()
            .map(|line| line.with_style(detail_style)),
        );
    }

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

/// issue #499：构造 header 前缀 meta span（`[role] provider/model — `）。
///
/// 格式（按可用宽度 `width` 截断，优先级 tool_name > model > role）：
/// - 都有：`[role] Provider/Model — tool_name`
/// - 仅 model：`Provider/Model — tool_name`
/// - 仅 role：`[role] — tool_name`
/// - 都无：返回空 Vec（header 不变）
///
/// 截断策略：当 meta + tool_name 预计超出 `width` 时，按优先级依次省略 role、model。
fn build_meta_spans(view: &ToolCallBlockView, width: usize) -> Vec<Span<'static>> {
    let role = view.role.as_deref();
    // model_id 已是 `Provider/Model` 格式（如 `Zhipu/glm-5.2`），直接复用，包含 provider 信息。
    let model = view.model_id.as_deref();

    if role.is_none() && model.is_none() {
        return Vec::new();
    }

    let tool_name_width = tool_display_name(&view.title).width();
    // 预留：tool_name + " — " 分隔符(3) + marker(2)
    let reserved = tool_name_width + 3 + 2;
    let remaining = width.saturating_sub(reserved);

    let role_seg = role.map(|r| format!("[{}] ", r)).unwrap_or_default();
    let model_seg = model.map(|m| format!("{} ", m)).unwrap_or_default();
    let full_meta = format!("{}{}", role_seg, model_seg);
    let full_meta_width = full_meta.width();

    let mut spans: Vec<Span<'static>> = Vec::new();
    if full_meta_width <= remaining {
        // 完整显示：role 段用 YELLOW，model 段用 TEXT_MUTED。
        if let Some(r) = role {
            spans.push(Span::styled(
                format!("[{}] ", r),
                Style::default().fg(theme::YELLOW),
            ));
        }
        if let Some(m) = model {
            spans.push(Span::styled(
                format!("{} ", m),
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }
    } else if !model_seg.is_empty() {
        // 省略 role，仅显示 model（若仍超长则截断 + …）。
        let seg = if model_seg.width() > remaining {
            let truncated = truncate_unicode(model.unwrap_or(""), remaining.saturating_sub(2));
            format!("{}… ", truncated)
        } else {
            model_seg
        };
        spans.push(Span::styled(seg, Style::default().fg(theme::TEXT_MUTED)));
    } else {
        // 只有 role：截断 + …。
        let truncated = truncate_unicode(role.unwrap_or(""), remaining.saturating_sub(2));
        spans.push(Span::styled(
            format!("[{}]… ", truncated),
            Style::default().fg(theme::YELLOW),
        ));
    }
    spans.push(Span::styled("— ", Style::default().fg(theme::TEXT_MUTED)));
    spans
}

/// 按字符宽度截断 unicode 字符串，不会切断多字节字符的中间部分。
fn truncate_unicode(s: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut out = String::new();
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + cw > max_width {
            break;
        }
        width += cw;
        out.push(ch);
    }
    out
}

/// 将 meta spans 前置到 header line（保持其它 span 顺序）。
fn prepend_spans(mut line: Line<'static>, prefix: Vec<Span<'static>>) -> Line<'static> {
    let mut new_spans = prefix;
    new_spans.append(&mut line.spans);
    line.spans = new_spans;
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::output::ToolSemanticStatus;
    use crate::tui::view_model::style::SemanticStyle;
    use unicode_width::UnicodeWidthStr;

    fn tool(status: ToolSemanticStatus) -> ToolCallBlockView {
        ToolCallBlockView {
            key: "t1".into(),
            chat_id: None,
            turn_id: None,
            tool_call_id: Some("t1".into()),
            title: "Grep".into(),
            icon: "●".into(),
            semantic_status: status,
            style: SemanticStyle::Running,
            args_preview: Some("/foo/".into()),
            activity_summary: None,
            result_summary: None,
            result_payload: None,
            workspace_root: None,
            collapsible: false,
            collapsed: false,
            provider_id: None,
            model_id: None,
            role: None,
        }
    }

    #[test]
    fn test_tool_call_running_applies_text_color_to_title() {
        // marker（●）现由 gutter 注入；header 文本统一使用 TEXT 色（与 assistant message 一致），
        // 任务状态由 gutter 颜色表示。颜色通过 RenderedLine 的 line base style 传递。
        let block = render_tool_call(
            "t1",
            &tool(ToolSemanticStatus::Running),
            &RenderCtx { text_width: 80 },
        );
        // header 行的 line base style 应为 TEXT
        assert_eq!(block.lines[0].style.fg, Some(theme::TEXT));
        assert!(block.lines[0].plain.contains("Search"));
        // header 行不再自写 marker 字形（gutter.rs 覆盖 marker）。
        assert!(
            !block.lines[0].plain.starts_with('●'),
            "header 不应自写 ● marker"
        );
    }

    #[test]
    fn test_tool_call_success_uses_text_title_color() {
        let mut view = tool(ToolSemanticStatus::Success);
        view.style = SemanticStyle::Success;
        view.icon = "✓".into();
        let block = render_tool_call("t1", &view, &RenderCtx { text_width: 80 });
        // header 行的 line base style 应为 TEXT
        assert_eq!(block.lines[0].style.fg, Some(theme::TEXT));
        assert!(block.lines[0].plain.contains("Search"));
        assert!(
            !block.lines[0].plain.starts_with('✓'),
            "header 不应自写 ✓ marker"
        );
    }

    #[test]
    fn test_tool_call_renders_args_detail_from_summary() {
        // summary 提供工具入参 JSON，经 format_tool_call 产出 header + detail，
        // 验证参数预览作为 detail 行渲染（取代旧 OutputArea 命令式 push）。
        let mut view = tool(ToolSemanticStatus::Running);
        view.title = "Grep".into();
        view.args_preview = Some(r#"{"pattern":"test","path":"src"}"#.into());

        let block = render_tool_call("t1", &view, &RenderCtx { text_width: 80 });

        // header 含工具名
        assert!(block.lines[0].plain.contains("Search"));
        // Grep header 现在包含 pattern 和 path
        assert!(block.lines[0].plain.contains("test"));
    }

    #[test]
    fn test_tool_call_renders_args_detail_from_args_preview_before_summary() {
        let mut view = tool(ToolSemanticStatus::Running);
        view.title = "Grep".into();
        view.args_preview = Some(r#"{"pattern":"test","path":"src"}"#.into());

        let block = render_tool_call("t1", &view, &RenderCtx { text_width: 80 });

        assert!(block.lines[0].plain.contains("Search"));
        assert!(
            block.lines[0].plain.contains("test"),
            "ToolArgumentsDelta 后应不等 ToolResult/最终 summary 就显示 header"
        );
    }

    #[test]
    fn test_tool_call_renders_header_only_no_result_lines() {
        // 结果已升为独立子块（ToolResult），tool_call 仅渲染 header（+ args detail）。
        // 即使 result_summary 有值，也不应出现在本块内。
        let mut view = tool(ToolSemanticStatus::Success);
        view.title = "Bash".into();
        view.result_summary = Some("done: 3 matches".into());
        view.args_preview = None;

        let block = render_tool_call("t1", &view, &RenderCtx { text_width: 80 });

        assert_eq!(block.lines.len(), 1, "无 summary 时只应有 header 行");
        assert!(
            block
                .lines
                .iter()
                .all(|line| !line.plain.contains("done: 3 matches")),
            "结果文本不应出现在 tool_call 块内（已升为子块）"
        );
    }

    // ── issue #361 回归：tool_call 三部分（header / detail / activity）应消费
    // ctx.text_width 做 wrap，窄终端下不溢出。修前 _ctx 被忽略，长内容超出被截断。

    #[test]
    fn test_tool_call_wraps_long_header_to_text_width() {
        // 未注册工具的长 display name（如 MCP 工具 mcp__github__create_issue）走
        // format_tool_call fallback：header = "● {display_name}"，strip_leading_bullet
        // 去掉 "● "，header 实际为 display_name 本身。窄终端应 wrap 到 ctx.text_width。
        let mut view = tool(ToolSemanticStatus::Running);
        view.title = "mcp__github__create_issue_a_very_long_tool_name".into();
        view.args_preview = Some("{}".into()); // 触发 fallback header 路径

        let block = render_tool_call("t1", &view, &RenderCtx { text_width: 20 });

        assert!(!block.lines.is_empty(), "block 至少应有 header 行");
        for (i, line) in block.lines.iter().enumerate() {
            assert!(
                line.plain.width() <= 20,
                "header 行 #{i} 宽度 {} 超 20: {:?}",
                line.plain.width(),
                line.plain
            );
        }
    }

    #[test]
    fn test_tool_call_wraps_long_detail_lines_to_text_width() {
        // 未注册工具的长 args JSON 经 fallback detail（truncate_json ≤100 字符）渲染为
        // detail 行。窄终端应 wrap 而非整行溢出。
        let mut view = tool(ToolSemanticStatus::Running);
        view.title = "UnknownLongToolName".into();
        let long_value = "x".repeat(120);
        view.args_preview = Some(format!(r#"{{"key":"{long_value}"}}"#));

        let block = render_tool_call("t1", &view, &RenderCtx { text_width: 40 });

        assert!(
            block.lines.len() >= 2,
            "应有 header + detail 行，实际: {:?}",
            block
                .lines
                .iter()
                .map(|l| l.plain.as_str())
                .collect::<Vec<_>>()
        );
        for (i, line) in block.lines.iter().enumerate() {
            assert!(
                line.plain.width() <= 40,
                "行 #{i} 宽度 {} 超 40: {:?}",
                line.plain.width(),
                line.plain
            );
        }
    }

    #[test]
    fn test_tool_call_wraps_long_activity_summary_to_text_width() {
        // Agent 等长任务的 activity_summary 在窄终端应 wrap 而非溢出。
        let mut view = tool(ToolSemanticStatus::Running);
        view.title = "Bash".into();
        view.args_preview = Some(r#"{"command":"ls"}"#.into());
        view.activity_summary = Some(
            "子任务正在执行一个非常长的操作描述文本用于测试窄终端下 activity 行的换行行为".into(),
        );

        let block = render_tool_call("t1", &view, &RenderCtx { text_width: 30 });

        for (i, line) in block.lines.iter().enumerate() {
            assert!(
                line.plain.width() <= 30,
                "activity 行 #{i} 宽度 {} 超 30: {:?}",
                line.plain.width(),
                line.plain
            );
        }
    }
}

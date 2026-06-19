use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::output::tool_display::format_tool_call;
use crate::tui::render::theme;
use crate::tui::view_model::output::ToolCallBlockView;
use crate::tui::view_model::tool_name::tool_display_name;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// 渲染工具调用块：仅 header（标题）+ args detail 行 + 可选的 activity 状态行。
///
/// 工具结果已升为独立子块（`ToolResult` 变体，见 `blocks/tool_result.rs`，#60），
/// 由 assembler 作为本块的 depth-1 子节点附加，此处不再渲染结果。
pub fn render_tool_call(
    block_id: &str,
    view: &ToolCallBlockView,
    _ctx: &RenderCtx,
) -> RenderedBlock {
    let header_input = view.args_preview.as_deref().filter(|s| !s.is_empty());
    let (header_line, detail_lines) = header_input
        .map(|raw_json| format_tool_call(&view.title, raw_json, view.result_summary.as_deref()))
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
    // marker（●/✓/✗）现由 gutter 注入；header 只渲染去掉 format_tool_call 前导 ● 的标题文本。
    // header 文本颜色统一使用 TEXT（与 assistant message 一致），任务状态由 gutter 颜色表示。
    // 通过 RenderedLine 的 line base style 让未显式着色的 span 继承 TEXT 色，
    // 已有显式颜色的 span（如 Read 的 summary 灰色）保留各自样式。
    let header_line = strip_leading_bullet(header_line);
    let first_line =
        RenderedLine::new(header_line.spans).with_style(Style::default().fg(theme::TEXT));
    let mut lines = vec![first_line];
    for detail in detail_lines {
        lines.push(RenderedLine::new(vec![Span::styled(
            detail,
            Style::default().fg(theme::TEXT_MUTED),
        )]));
    }
    // 渲染 activity_summary：Agent 等长时间工具执行过程中显示当前进度（如子 agent 当前操作），
    // 嵌套在 ToolCall block 内而非根级 DiagnosticNotice 泄露到对话流中。
    if let Some(activity) = &view.activity_summary {
        lines.push(RenderedLine::new(vec![Span::styled(
            activity.clone(),
            Style::default().fg(theme::TEXT_MUTED),
        )]));
    }

    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::output::ToolSemanticStatus;
    use crate::tui::view_model::style::SemanticStyle;

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
            collapsible: false,
            collapsed: false,
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
}

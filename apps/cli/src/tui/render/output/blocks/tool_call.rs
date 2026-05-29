use crate::tui::render::output::blocks::diagnostic::semantic_color;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::output::tool_display::format_tool_call;
use crate::tui::render::theme;
use crate::tui::view_model::output::ToolCallBlockView;
use ratatui::style::Style;
use ratatui::text::Span;

/// 渲染工具调用块：仅 header（标题）+ args detail 行。
///
/// 工具结果已升为独立子块（`ToolResult` 变体，见 `blocks/tool_result.rs`，#60），
/// 由 assembler 作为本块的 depth-1 子节点附加，此处不再渲染结果。
pub fn render_tool_call(
    block_id: &str,
    view: &ToolCallBlockView,
    _ctx: &RenderCtx,
) -> RenderedBlock {
    let (header_text, detail_lines) = view
        .summary
        .as_deref()
        .map(|summary| format_tool_call(&view.title, summary))
        .unwrap_or_else(|| (format!("● {}", view.title), Vec::new()));
    let icon_color = semantic_color(view.style);
    // marker（●/✓/✗）现由 gutter 注入；header 只渲染去掉 format_tool_call 前导 ● 的标题文本（颜色不变）。
    let title_text = header_text
        .strip_prefix('●')
        .unwrap_or(&header_text)
        .trim_start();
    let mut lines = vec![RenderedLine::new(vec![Span::styled(
        title_text.to_string(),
        Style::default().fg(icon_color),
    )])];
    for detail in detail_lines {
        lines.push(RenderedLine::new(vec![Span::styled(
            detail,
            Style::default().fg(theme::TEXT_MUTED),
        )]));
    }

    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
    }
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
            summary: None,
            activity_summary: None,
            result_summary: None,
            collapsible: false,
            collapsed: false,
        }
    }

    #[test]
    fn test_tool_call_running_applies_theme_color_to_title() {
        // marker（●）现由 gutter 注入；组件只渲染带语义色的标题（无自写 icon span）。
        let block = render_tool_call(
            "t1",
            &tool(ToolSemanticStatus::Running),
            &RenderCtx { width: 80 },
        );
        let title_span = block.lines[0]
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("Grep"))
            .unwrap();

        assert_eq!(title_span.style.fg, Some(theme::TOOL_RUNNING));
        assert!(block.lines[0].plain.contains("Grep"));
        // header 行不再自写 marker 字形（gutter.rs 覆盖 marker）。
        assert!(
            !block.lines[0].plain.starts_with('●'),
            "header 不应自写 ● marker"
        );
    }

    #[test]
    fn test_tool_call_success_uses_success_title_color() {
        let mut view = tool(ToolSemanticStatus::Success);
        view.style = SemanticStyle::Success;
        view.icon = "✓".into();
        let block = render_tool_call("t1", &view, &RenderCtx { width: 80 });
        let title_span = block.lines[0]
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("Grep"))
            .unwrap();

        assert_eq!(title_span.style.fg, Some(theme::SUCCESS));
        assert!(block.lines[0].plain.contains("Grep"));
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
        view.title = "Read".into();
        view.summary = Some(r#"{"file_path":"src/lib.rs"}"#.into());

        let block = render_tool_call("t1", &view, &RenderCtx { width: 80 });

        // header 含工具名
        assert!(block.lines[0].plain.contains("Read"));
        // detail 行含文件路径参数
        let has_path = block
            .lines
            .iter()
            .any(|line| line.plain.contains("src/lib.rs"));
        assert!(has_path, "参数预览应作为 detail 行渲染");
    }

    #[test]
    fn test_tool_call_renders_header_only_no_result_lines() {
        // 结果已升为独立子块（ToolResult），tool_call 仅渲染 header（+ args detail）。
        // 即使 result_summary 有值，也不应出现在本块内。
        let mut view = tool(ToolSemanticStatus::Success);
        view.title = "Bash".into();
        view.result_summary = Some("done: 3 matches".into());

        let block = render_tool_call("t1", &view, &RenderCtx { width: 80 });

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

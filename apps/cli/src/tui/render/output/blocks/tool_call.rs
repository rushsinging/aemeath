use crate::tui::render::output::blocks::diagnostic::semantic_color;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::output::tool_display::format_tool_call;
use crate::tui::render::output_area::INDENT;
use crate::tui::render::theme;
use crate::tui::view_model::output::ToolCallBlockView;
use ratatui::style::{Color, Style};
use ratatui::text::Span;

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
    let header_text = header_text.replacen('●', &view.icon, 1);
    let icon_color = semantic_color(view.style);
    let mut lines = vec![RenderedLine::new(vec![
        Span::styled(format!("{} ", view.icon), Style::default().fg(icon_color)),
        Span::styled(
            header_text
                .strip_prefix(&view.icon)
                .unwrap_or(&header_text)
                .trim_start()
                .to_string(),
            Style::default().fg(icon_color),
        ),
    ])];
    for detail in detail_lines {
        lines.push(RenderedLine::new(vec![Span::styled(
            format!("{INDENT}{detail}"),
            Style::default().fg(theme::TEXT_MUTED),
        )]));
    }
    for detail in [&view.activity_summary, &view.result_summary]
        .into_iter()
        .flatten()
    {
        let color = if detail == view.result_summary.as_ref().unwrap_or(detail) {
            theme::TEXT_DIM
        } else {
            theme::TEXT
        };
        for line in format_result_lines(&view.title, detail, color) {
            lines.push(line);
        }
    }

    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
    }
}

fn format_result_lines(tool_name: &str, result: &str, color: Color) -> Vec<RenderedLine> {
    if result.trim().is_empty() {
        return Vec::new();
    }
    let max_lines = if tool_name == "TaskListComplete" {
        0
    } else {
        5
    };
    let total = result.lines().count();
    let style = Style::default().fg(color);
    let mut out = Vec::new();
    for line in result.lines().take(max_lines) {
        out.push(RenderedLine::new(vec![Span::styled(
            format!("{INDENT}{line}"),
            style,
        )]));
    }
    if total > max_lines {
        out.push(RenderedLine::new(vec![Span::styled(
            format!("{INDENT}... ({} lines omitted)", total - max_lines),
            style,
        )]));
    }
    out
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
    fn test_tool_call_running_applies_theme_color_to_icon_and_title() {
        let block = render_tool_call(
            "t1",
            &tool(ToolSemanticStatus::Running),
            &RenderCtx { width: 80 },
        );
        let icon_span = block.lines[0]
            .spans
            .iter()
            .find(|span| span.content.as_ref() == "● ")
            .unwrap();
        let title_span = block.lines[0]
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("Grep"))
            .unwrap();

        assert_eq!(icon_span.style.fg, Some(theme::TOOL_RUNNING));
        assert_eq!(title_span.style.fg, Some(theme::TOOL_RUNNING));
        assert!(block.lines[0].plain.contains("Grep"));
    }

    #[test]
    fn test_tool_call_success_uses_success_icon_color() {
        let mut view = tool(ToolSemanticStatus::Success);
        view.style = SemanticStyle::Success;
        view.icon = "✓".into();
        let block = render_tool_call("t1", &view, &RenderCtx { width: 80 });
        let icon_span = block.lines[0]
            .spans
            .iter()
            .find(|span| span.content.as_ref() == "✓ ")
            .unwrap();
        let title_span = block.lines[0]
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("Grep"))
            .unwrap();

        assert_eq!(icon_span.style.fg, Some(theme::SUCCESS));
        assert_eq!(title_span.style.fg, Some(theme::SUCCESS));
        assert!(block.lines[0].plain.contains("Grep"));
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
    fn test_tool_call_renders_result_summary_lines() {
        // result_summary 应作为结果行渲染，验证结果展示已迁移到新组件。
        let mut view = tool(ToolSemanticStatus::Success);
        view.result_summary = Some("done: 3 matches".into());

        let block = render_tool_call("t1", &view, &RenderCtx { width: 80 });

        let has_result = block
            .lines
            .iter()
            .any(|line| line.plain.contains("done: 3 matches"));
        assert!(has_result, "result_summary 应渲染为结果行");
    }

    #[test]
    fn test_tool_call_deduplicates_repeated_completion_summary() {
        let mut view = tool(ToolSemanticStatus::Success);
        view.style = SemanticStyle::Success;
        view.icon = "✓".into();
        view.title = "Read".into();
        view.summary = Some(r#"{"file_path":"docs/bug/active.md"}"#.into());
        view.result_summary = Some("✓ Read completed".into());

        let block = render_tool_call("t1", &view, &RenderCtx { width: 80 });
        let completed_count = block
            .lines
            .iter()
            .filter(|line| line.plain.contains("✓ Read completed"))
            .count();

        assert_eq!(completed_count, 1, "完成摘要不应同时出现在标题和结果行");
    }
}

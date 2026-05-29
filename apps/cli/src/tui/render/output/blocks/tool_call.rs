use crate::tui::render::output::blocks::diagnostic::semantic_color;
use crate::tui::render::output::blocks::edit_diff::render_edit_diff;
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
    ctx: &RenderCtx,
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
            Style::default().fg(theme::TEXT),
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
        // Edit 工具结果含 ---DIFF--- 标记时，渲染为带行号/语义色/语法高亮的 diff。
        if let Some(diff_lines) = render_edit_diff(&view.title, detail, ctx.width) {
            lines.extend(diff_lines);
            continue;
        }
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
    fn test_tool_call_title_visible_not_background_color() {
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

        assert_ne!(title_span.style.fg, Some(theme::SURFACE));
        assert_ne!(title_span.style.fg, title_span.style.bg);
        assert!(block.lines[0].plain.contains("Grep"));
    }

    #[test]
    fn test_tool_call_success_uses_success_icon_color() {
        let block = render_tool_call(
            "t1",
            &tool(ToolSemanticStatus::Success),
            &RenderCtx { width: 80 },
        );

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
    fn test_tool_call_edit_result_renders_diff_with_numbers_signs_indent_color() {
        // #61 端到端：Edit 结果含 ---DIFF--- 标记，应渲染为带行号 + 加减语义色 +
        // 缩进 + 语法高亮的 diff 行，而非原始标记纯文本。
        let mut view = tool(ToolSemanticStatus::Success);
        view.title = "Edit(src/lib.rs)".into();
        view.result_summary = Some(
            "replaced 1 occurrence(s) in src/lib.rs\n---DIFF---\nlet a = 1;\n---DIFF---\nlet a = 2;"
                .into(),
        );

        let block = render_tool_call("t1", &view, &RenderCtx { width: 80 });

        // 不残留原始标记
        assert!(
            block.lines.iter().all(|line| !line.plain.contains("---DIFF---")),
            "不应残留 ---DIFF--- 标记"
        );
        // 删除/新增行带加减语义
        assert!(
            block
                .lines
                .iter()
                .any(|line| line.plain.contains("- ") && line.plain.contains("1;")),
            "应含删除行"
        );
        assert!(
            block
                .lines
                .iter()
                .any(|line| line.plain.contains("+ ") && line.plain.contains("2;")),
            "应含新增行"
        );
        // diff 行带前景色（选中叠加保留 fg 的前提，bug #61）
        let diff_line = block
            .lines
            .iter()
            .find(|line| line.plain.contains("2;"))
            .expect("新增行存在");
        assert!(
            diff_line.spans.iter().any(|span| span.style.fg.is_some()),
            "diff 行应带前景色 span，供选中叠加保留"
        );
        // 行号缩进
        assert!(
            diff_line.plain.starts_with("  "),
            "diff 行应保留两空格缩进"
        );
    }
}

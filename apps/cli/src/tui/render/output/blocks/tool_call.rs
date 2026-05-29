use crate::tui::render::output::blocks::diagnostic::semantic_color;
use crate::tui::render::output::blocks::edit_diff::render_edit_diff;
use crate::tui::render::output::primitives::fenced::render_fenced_markdown;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::output::tool_display::{format_tool_call, result_max_lines};
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
    for detail in [&view.activity_summary, &view.result_summary]
        .into_iter()
        .flatten()
    {
        // Edit 工具结果含 ---DIFF--- 标记时，渲染为带行号/语义色/语法高亮的 diff。
        // ext 从 summary（入参 JSON 含 file_path）推断，而非裸 title="Edit"（M1）。
        if let Some(diff_lines) = render_edit_diff(view.summary.as_deref(), detail, ctx.width) {
            lines.extend(diff_lines);
            continue;
        }
        let color = if detail == view.result_summary.as_ref().unwrap_or(detail) {
            theme::TEXT_DIM
        } else {
            theme::TEXT
        };
        for line in format_result_lines(&view.title, detail, color, ctx.width) {
            lines.push(line);
        }
    }

    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
    }
}

/// 渲染普通工具结果（非 Edit diff）：解析 fenced code block / markdown / 表格，
/// 再按工具注册的 `result_max_lines` 截断。
///
/// fence/markdown 解析复用 `primitives::fenced`（与 assistant 共用，DRY），
/// 因状态机随调用销毁，fence 结束后普通行恢复正常色，结构上隔离 #65。
/// 截断行数取自 `ToolDisplay::result_max_lines`（未注册的工具回退默认值）。
fn format_result_lines(
    tool_name: &str,
    result: &str,
    color: Color,
    width: u16,
) -> Vec<RenderedLine> {
    if result.trim().is_empty() {
        return Vec::new();
    }
    let max_lines = result_max_lines(tool_name);
    let base = Style::default().fg(color);
    // render_fenced_markdown 现产无缩进行（#60）；块级缩进由 gutter 在组合期注入，
    // 此处不再自拼 INDENT（结果行属于 tool_call block 的后续行，gutter 给等宽空白）。
    let rendered: Vec<RenderedLine> = render_fenced_markdown(result, base, width);
    let total = rendered.len();
    let mut out: Vec<RenderedLine> = rendered.into_iter().take(max_lines).collect();
    if total > max_lines {
        out.push(RenderedLine::new(vec![Span::styled(
            format!("... ({} lines omitted)", total - max_lines),
            base,
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
    fn test_tool_call_result_fence_does_not_leak_code_color_after_close() {
        // #65 回归：工具结果含完整 ```fenced block```，代码块结束后的普通行
        // 不得残留 CODE 色（fence 状态机随 block 渲染销毁，结构上隔离泄漏）；
        // 且代码行本身应为 CODE 色。
        let mut view = tool(ToolSemanticStatus::Success);
        view.title = "Bash".into();
        view.result_summary = Some("```\ncode\n```\nafter".into());

        let block = render_tool_call("t1", &view, &RenderCtx { width: 80 });

        let code = block
            .lines
            .iter()
            .find(|l| l.plain.contains("code") && !l.plain.contains("```"))
            .expect("代码行存在");
        assert!(
            code.spans.iter().any(|s| s.style.fg == Some(theme::CODE)),
            "fence 内代码行应为 CODE 色"
        );

        let after = block
            .lines
            .iter()
            .find(|l| l.plain.contains("after"))
            .expect("围栏后普通行存在");
        assert!(
            after.spans.iter().all(|s| s.style.fg != Some(theme::CODE)),
            "fence 结束后普通行不应残留 CODE 色（#65）"
        );
    }

    #[test]
    fn test_tool_call_result_unclosed_fence_does_not_panic() {
        // 边界：无闭合 fence 的结果不应 panic，且能产出代码行。
        let mut view = tool(ToolSemanticStatus::Success);
        view.title = "Bash".into();
        view.result_summary = Some("```\nline1\nline2".into());

        let block = render_tool_call("t1", &view, &RenderCtx { width: 80 });

        assert!(block.lines.iter().any(|l| l.plain.contains("line1")
            && l.spans.iter().any(|s| s.style.fg == Some(theme::CODE))));
    }

    #[test]
    fn test_tool_call_result_max_lines_uses_tool_display_zero() {
        // 边界：result_max_lines==0 的工具（TaskListComplete）结果不渲染结果行，
        // 仅可能出现 "lines omitted" 提示。验证截断行数取自 ToolDisplay 而非硬编码。
        let mut view = tool(ToolSemanticStatus::Success);
        view.title = "TaskListComplete".into();
        view.result_summary = Some("a\nb\nc".into());

        let block = render_tool_call("t1", &view, &RenderCtx { width: 80 });

        assert!(
            block.lines.iter().all(|l| l.plain != "a"),
            "max_lines=0 时不应渲染结果内容行"
        );
        assert!(
            block
                .lines
                .iter()
                .any(|l| l.plain.contains("lines omitted")),
            "应出现省略提示"
        );
    }

    #[test]
    fn test_tool_call_empty_result_renders_no_result_lines() {
        // 错误/空路径：空白结果不产出结果行（仅 header）。
        let mut view = tool(ToolSemanticStatus::Success);
        view.title = "Bash".into();
        view.result_summary = Some("   \n  ".into());

        let block = render_tool_call("t1", &view, &RenderCtx { width: 80 });

        assert_eq!(block.lines.len(), 1, "空结果只应有 header 行");
    }

    #[test]
    fn test_tool_call_edit_result_renders_diff_with_numbers_signs_indent_color() {
        // #61 端到端：Edit 结果含 ---DIFF--- 标记，应渲染为带行号 + 加减语义色 +
        // 缩进 + 语法高亮的 diff 行，而非原始标记纯文本。
        // M1：使用运行时真实的裸 title "Edit"（无括号路径）+ summary 含 file_path，
        // ext 必须从 summary 推断而非 title。
        let mut view = tool(ToolSemanticStatus::Success);
        view.title = "Edit".into();
        view.summary = Some(r#"{"file_path":"src/lib.rs"}"#.into());
        view.result_summary = Some(
            "replaced 1 occurrence(s) in src/lib.rs\n---DIFF---\nlet a = 1;\n---DIFF---\nlet a = 2;"
                .into(),
        );

        let block = render_tool_call("t1", &view, &RenderCtx { width: 80 });

        // 不残留原始标记
        assert!(
            block
                .lines
                .iter()
                .all(|line| !line.plain.contains("---DIFF---")),
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
        assert!(diff_line.plain.starts_with("  "), "diff 行应保留两空格缩进");
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

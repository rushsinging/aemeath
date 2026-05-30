//! 工具结果子块渲染组件：独占工具结果的富渲染（从 tool_call.rs 迁移而来，#60）。
//!
//! 作为 ToolCall 的 depth-1 子节点，结果行不再自拼缩进/marker——块级缩进由
//! gutter 在组合期注入（续行等宽空白），结构上隔离 #65 的 fence 状态机泄漏。

use crate::tui::render::output::blocks::edit_diff::render_edit_diff;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::output::tool_display::{result_max_lines, result_render_kind, ResultRender};
use crate::tui::render::theme;
use crate::tui::view_model::output::ToolResultBlockView;
use ratatui::style::Style;
use ratatui::text::Span;

pub fn render_tool_result(
    block_id: &str,
    view: &ToolResultBlockView,
    ctx: &RenderCtx,
) -> RenderedBlock {
    // result 渲染类型由工具显式声明（`ToolDisplay::result_render`），渲染层据此分发——
    // 不按 `---DIFF---` 字符或硬编码工具名猜测。
    let lines = match result_render_kind(&view.tool_title) {
        // Edit：解析 `---DIFF---` 渲染加减色 diff；解析失败回退纯文本预览。
        ResultRender::Diff => {
            render_edit_diff(view.summary.as_deref(), &view.result_text, ctx.width)
                .unwrap_or_else(|| format_result_lines(&view.tool_title, &view.result_text))
        }
        // Plain：纯文本原样预览（Read/Bash 等）。
        ResultRender::Plain => format_result_lines(&view.tool_title, &view.result_text),
    };

    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
    }
}

/// 渲染 Plain 工具结果：**纯文本原样**逐行，按 `result_max_lines` 截断。
///
/// 用暗色（`theme::TEXT_DIM`）——文件/命令输出预览不跟随 tool 状态色（状态绿/红只在 header
/// 的 ✓/✗ marker）；**不做 markdown 重渲染**——避免文件内容里的 markdown（表格/标题/fence）
/// 被渲染变形，保留原文（含 Read 行号/缩进，#91）。
fn format_result_lines(tool_name: &str, result: &str) -> Vec<RenderedLine> {
    if result.trim().is_empty() {
        return Vec::new();
    }
    let max_lines = result_max_lines(tool_name);
    // max_lines==0 的工具（如 AskUserQuestion，答案已 echo）result 子块整体为空。
    if max_lines == 0 {
        return Vec::new();
    }
    let base = Style::default().fg(theme::TEXT_DIM);
    let lines: Vec<&str> = result.lines().collect();
    let total = lines.len();
    let mut out: Vec<RenderedLine> = lines
        .iter()
        .take(max_lines)
        .map(|line| RenderedLine::new(vec![Span::styled((*line).to_string(), base)]))
        .collect();
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
    use crate::tui::render::theme;
    use crate::tui::view_model::output::ToolResultBlockView;

    use crate::tui::view_model::style::SemanticStyle;

    fn result(tool_title: &str, result_text: &str) -> ToolResultBlockView {
        ToolResultBlockView {
            key: format!("{tool_title}-result"),
            tool_title: tool_title.into(),
            summary: None,
            result_text: result_text.into(),
            style: SemanticStyle::Success,
        }
    }

    #[test]
    fn test_render_tool_result_renders_result_text_lines() {
        // 正常路径：result_text 应作为结果行渲染。
        let view = result("Grep", "done: 3 matches");
        let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

        assert_eq!(block.block_id, "t1-result");
        assert!(block
            .lines
            .iter()
            .any(|line| line.plain.contains("done: 3 matches")));
    }

    #[test]
    fn test_render_tool_result_non_edit_diff_marker_kept_as_plain_text() {
        // #64×#90 回归：非 Edit 工具（Read）result 含 ---DIFF--- 文本（如读到描述 diff
        // 格式的文档/源码）不得被误解析为 diff，应按普通预览保留原文。
        let view = result("Read", "intro\n---DIFF---\nold\n---DIFF---\nnew");
        let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

        assert!(
            block.lines.iter().any(|l| l.plain.contains("---DIFF---")),
            "非 Edit 工具应保留 ---DIFF--- 原文（不渲染为 diff），got: {:?}",
            block.lines.iter().map(|l| l.plain.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_render_tool_result_plain_keeps_fence_markers_as_text_in_dim() {
        // Plain 纯文本原样（#91）：result 里的 ``` fence、code、after 全作普通文本保留，
        // 不做 markdown 重渲染（无 CODE 色），整体用暗色 TEXT_DIM（不跟随状态绿/红）。
        let view = result("Bash", "```\ncode\n```\nafter");
        let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

        assert!(
            block.lines.iter().any(|l| l.plain.contains("```")),
            "fence 标记应作普通文本原样保留"
        );
        assert!(block.lines.iter().any(|l| l.plain == "code"));
        assert!(block.lines.iter().any(|l| l.plain == "after"));
        assert!(
            block
                .lines
                .iter()
                .all(|l| l.spans.iter().all(|s| s.style.fg == Some(theme::TEXT_DIM))),
            "Plain 预览整体用暗色 TEXT_DIM，不渲染 fence CODE 色、不跟随状态色"
        );
    }

    #[test]
    fn test_render_tool_result_plain_unclosed_fence_does_not_panic() {
        // 边界：纯文本预览对无闭合 fence 不 panic，原样逐行保留。
        let view = result("Bash", "```\nline1\nline2");

        let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

        assert!(block.lines.iter().any(|l| l.plain == "line1"));
        assert!(block.lines.iter().any(|l| l.plain == "line2"));
    }

    #[test]
    fn test_render_tool_result_max_lines_zero_renders_nothing() {
        // 边界：result_max_lines==0 的工具（TaskListComplete/AskUserQuestion）result 子块
        // 整体为空——既不渲染内容行，也不显示 "lines omitted" 提示。
        let view = result("TaskListComplete", "a\nb\nc");

        let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

        assert!(block.lines.is_empty(), "max_lines=0 时 result 子块应为空");
    }

    #[test]
    fn test_render_tool_result_empty_result_renders_no_lines() {
        // 错误/空路径：空白结果不产出任何行。
        let view = result("Bash", "   \n  ");

        let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

        assert!(block.lines.is_empty(), "空结果不应产出结果行");
    }

    #[test]
    fn test_render_tool_result_edit_diff_renders_with_numbers_signs_indent_color() {
        // #61 端到端：Edit 结果含 ---DIFF--- 标记，应渲染为带行号 + 加减语义色 +
        // 缩进 + 语法高亮的 diff 行，而非原始标记纯文本；ext 从 summary 推断。
        let mut view = result(
            "Edit",
            "replaced 1 occurrence(s) in src/lib.rs\n---DIFF---\nlet a = 1;\n---DIFF---\nlet a = 2;",
        );
        view.summary = Some(r#"{"file_path":"src/lib.rs"}"#.into());

        let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

        assert!(
            block
                .lines
                .iter()
                .all(|line| !line.plain.contains("---DIFF---")),
            "不应残留 ---DIFF--- 标记"
        );
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
        let diff_line = block
            .lines
            .iter()
            .find(|line| line.plain.contains("2;"))
            .expect("新增行存在");
        assert!(
            diff_line.spans.iter().any(|span| span.style.fg.is_some()),
            "diff 行应带前景色 span，供选中叠加保留"
        );
        assert!(diff_line.plain.starts_with("  "), "diff 行应保留两空格缩进");
    }
}

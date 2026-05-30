//! 工具结果子块渲染组件：独占工具结果的富渲染（从 tool_call.rs 迁移而来，#60）。
//!
//! 作为 ToolCall 的 depth-1 子节点，结果行不再自拼缩进/marker——块级缩进由
//! gutter 在组合期注入（续行等宽空白），结构上隔离 #65 的 fence 状态机泄漏。

use crate::tui::render::output::blocks::edit_diff::render_edit_diff;
use crate::tui::render::output::primitives::fenced::render_fenced_markdown;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::output::tool_display::{result_max_lines, result_renders_as_diff};
use crate::tui::render::output::blocks::diagnostic::semantic_color;
use crate::tui::view_model::output::ToolResultBlockView;
use ratatui::style::{Color, Style};
use ratatui::text::Span;

pub fn render_tool_result(
    block_id: &str,
    view: &ToolResultBlockView,
    ctx: &RenderCtx,
) -> RenderedBlock {
    // result 渲染类型由工具显式声明（`ToolDisplay::renders_result_as_diff`），渲染层据此分发，
    // 不按 `---DIFF---` 字符或硬编码工具名猜测：#64 后非 Edit 工具（如 Read）的 result 携带
    // 文件原文，文件内容巧含 `---DIFF---`（如描述 diff 格式的文档/源码）不得被误解析为 diff。
    let result_color = semantic_color(view.style);
    let diff_lines = result_renders_as_diff(&view.tool_title)
        .then(|| render_edit_diff(view.summary.as_deref(), &view.result_text, ctx.width))
        .flatten();
    let lines = if let Some(diff_lines) = diff_lines {
        diff_lines
    } else {
        // 结果行颜色跟随 tool call 状态（Success=绿, Error=红, Running=橙）。
        format_result_lines(&view.tool_title, &view.result_text, result_color, ctx.width)
    };

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
    // max_lines==0 的工具（如 AskUserQuestion，答案已 echo）不展示任何结果内容，
    // 也不显示 "lines omitted" 提示——result 子块整体为空。
    if max_lines == 0 {
        return Vec::new();
    }
    let base = Style::default().fg(color);
    // render_fenced_markdown 现产无缩进行（#60）；块级缩进由 gutter 在组合期注入，
    // 此处不再自拼 INDENT（结果作为 tool_call 的子块，gutter 给等宽空白）。
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
    fn test_render_tool_result_fence_does_not_leak_code_color_after_close() {
        // #65 回归：结果含完整 ```fenced block```，代码块结束后的普通行不得残留
        // CODE 色（fence 状态机随 block 渲染销毁，结构上隔离泄漏）；代码行本身应为 CODE 色。
        let mut view = result("Bash", "```\ncode\n```\nafter");
        view.tool_title = "Bash".into();

        let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

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
    fn test_render_tool_result_unclosed_fence_does_not_panic() {
        // 边界：无闭合 fence 的结果不应 panic，且能产出代码行。
        let view = result("Bash", "```\nline1\nline2");

        let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

        assert!(block.lines.iter().any(|l| l.plain.contains("line1")
            && l.spans.iter().any(|s| s.style.fg == Some(theme::CODE))));
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

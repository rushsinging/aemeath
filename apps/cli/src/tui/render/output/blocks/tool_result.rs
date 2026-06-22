//! 工具结果子块渲染组件：独占工具结果的富渲染（从 tool_call.rs 迁移而来，#60）。
//!
//! 作为 ToolCall 的 depth-1 子节点，结果行不再自拼缩进/marker——块级缩进由
//! gutter 在组合期注入（续行等宽空白），结构上隔离 #65 的 fence 状态机泄漏。

use crate::tui::render::output::blocks::edit_diff::render_edit_diff;
use crate::tui::render::output::primitives::wrap::{wrap_spans_with_prefix, WrapMode};
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::output::tool_display::{result_policy, ResultPolicy, ResultRender};
use crate::tui::render::theme;
use crate::tui::view_model::output::ToolResultBlockView;
use ratatui::style::Style;
use ratatui::text::Span;
use serde_json::Value;
use std::rc::Rc;

const OMITTED_LINE_COUNT_LIMIT: usize = 10_000;

/// 从结构化 JSON content 中提取显示文本。
/// 优先级：display > message > text > 序列化 JSON
fn display_text_from_json(content: &Value) -> Option<String> {
    if let Some(display) = content.get("display").and_then(|v| v.as_str()) {
        return Some(display.to_string());
    }
    if let Some(message) = content.get("message").and_then(|v| v.as_str()) {
        return Some(message.to_string());
    }
    if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
        return Some(text.to_string());
    }
    None
}

/// 尝试将 result_text 解析为结构化 JSON 并提取显示文本。
/// 如果解析失败或没有合适的字段，返回原始 result_text。
fn resolve_display_text(result_text: &str) -> String {
    // 尝试解析为 JSON
    if let Ok(content) = serde_json::from_str::<Value>(result_text) {
        if let Some(display) = display_text_from_json(&content) {
            return display;
        }
    }
    result_text.to_string()
}

pub fn render_tool_result(
    block_id: &str,
    view: &ToolResultBlockView,
    ctx: &RenderCtx,
) -> RenderedBlock {
    let policy = result_policy(&view.tool_title);
    // 解析结构化 JSON，提取显示文本
    let display_text = resolve_display_text(&view.result_text);
    crate::tui::log_debug!(
        "render tool_result block_id={} tool_title={} result_len={} display_len={} width={} style={:?} policy={:?}",
        block_id,
        view.tool_title,
        view.result_text.len(),
        display_text.len(),
        ctx.text_width,
        view.style,
        policy,
    );

    let lines = match policy {
        ResultPolicy::Hidden => vec![],
        ResultPolicy::Visible {
            max_lines,
            render_kind,
            tail_mode,
        } => {
            let limit = max_lines.unwrap_or(usize::MAX);
            match render_kind {
                ResultRender::Diff => {
                    render_edit_diff(view.args_preview.as_deref(), &display_text, ctx.text_width)
                        .unwrap_or_else(|| {
                            format_result_lines(
                                &view.tool_title,
                                &display_text,
                                ctx.text_width,
                                limit,
                            )
                        })
                }
                ResultRender::Plain => {
                    if tail_mode {
                        format_result_lines_tail(
                            &view.tool_title,
                            &display_text,
                            ctx.text_width,
                            limit,
                        )
                    } else {
                        format_result_lines(&view.tool_title, &display_text, ctx.text_width, limit)
                    }
                }
            }
        }
    };

    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(lines),
    }
}

/// 渲染 Plain 工具结果：**纯文本原样**逐行，按 `max_lines` 截断。
///
/// 用暗色（`theme::TEXT_DIM`）——文件/命令输出预览不跟随 tool 状态色（状态绿/红只在 header
/// 的 ✓/✗ marker）；**不做 markdown 重渲染**——避免文件内容里的 markdown（表格/标题/fence）
/// 被渲染变形，保留原文（含 Read 行号/缩进，#91）。
fn format_result_lines(
    _tool_name: &str,
    result: &str,
    width: u16,
    max_lines: usize,
) -> Vec<RenderedLine> {
    if result.trim().is_empty() {
        return Vec::new();
    }
    if max_lines == 0 {
        return Vec::new();
    }
    let base = Style::default().fg(theme::TEXT_DIM);
    let mut iter = result.lines();
    let mut out: Vec<RenderedLine> = Vec::new();
    // 逐原始行处理，每行 wrap 后累计渲染行数，达到 max_lines 即停止
    while out.len() < max_lines {
        match iter.next() {
            None => break,
            Some(line) => {
                out.extend(wrap_spans_with_prefix(
                    vec![Span::styled(line.to_string(), base)],
                    width as usize,
                    None,
                    WrapMode::Word,
                ));
            }
        }
    }
    // 截断 wrap 展开后超出 max_lines 的渲染行（处理单行长输出场景）
    if out.len() > max_lines {
        out.truncate(max_lines);
    }
    let omitted = iter.by_ref().take(OMITTED_LINE_COUNT_LIMIT + 1).count();
    if omitted > 0 {
        let omitted_label = if omitted > OMITTED_LINE_COUNT_LIMIT {
            format!("{OMITTED_LINE_COUNT_LIMIT}+")
        } else {
            omitted.to_string()
        };
        out.push(RenderedLine::new(vec![Span::styled(
            format!("... ({omitted_label} lines omitted)"),
            base,
        )]));
    }
    out
}

/// 渲染 Plain 工具结果（tail 模式）：只显示最后 `max_lines` 行。
/// 适用于 Bash 等持续输出的工具，用户关注最新输出。
fn format_result_lines_tail(
    _tool_name: &str,
    result: &str,
    width: u16,
    max_lines: usize,
) -> Vec<RenderedLine> {
    if result.trim().is_empty() {
        return Vec::new();
    }
    if max_lines == 0 {
        return Vec::new();
    }
    let base = Style::default().fg(theme::TEXT_DIM);
    let all_lines: Vec<&str> = result.lines().collect();
    let start = all_lines.len().saturating_sub(max_lines);
    let mut out: Vec<RenderedLine> = Vec::new();
    // get(start..) 返回 Option，不会 panic
    if let Some(remaining) = all_lines.get(start..) {
        for line in remaining {
            out.extend(wrap_spans_with_prefix(
                vec![Span::styled(line.to_string(), base)],
                width as usize,
                None,
                WrapMode::Word,
            ));
        }
    }
    // 截断 wrap 展开后超出 max_lines 的渲染行
    if out.len() > max_lines {
        out.truncate(max_lines);
    }
    // 显示省略的行数
    if start > 0 {
        out.insert(
            0,
            RenderedLine::new(vec![Span::styled(
                format!("... ({start} lines above)"),
                base,
            )]),
        );
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
            args_preview: None,
            result_text: result_text.into(),
            style: SemanticStyle::Success,
        }
    }

    #[test]
    fn test_render_tool_result_renders_result_text_lines() {
        // 正常路径：result_text 应作为结果行渲染。
        let view = result("Grep", "done: 3 matches");
        let block = render_tool_result("t1-result", &view, &RenderCtx { text_width: 80 });

        assert_eq!(block.block_id, "t1-result");
        assert!(block
            .lines
            .iter()
            .any(|line| line.plain.contains("done: 3 matches")));
    }

    #[test]
    fn test_render_tool_result_plain_wraps_long_lines_to_render_width() {
        let view = result("Bash", "abcdef");
        let block = render_tool_result("t1-result", &view, &RenderCtx { text_width: 4 });

        assert_eq!(block.lines[0].plain, "abcd");
        assert_eq!(block.lines[1].plain, "ef");
        assert!(block.lines[..2].iter().all(|line| line
            .spans
            .iter()
            .all(|span| span.style.fg == Some(theme::TEXT_DIM))));
    }

    #[test]
    fn test_render_tool_result_non_edit_diff_marker_kept_as_plain_text() {
        // #64×#90 回归：非 Edit 工具（Read）result 含 ---DIFF--- 文本（如读到描述 diff
        // 格式的文档/源码）不得被误解析为 diff，应按普通预览保留原文。
        // Read 的 result 策略现在是 Hidden，所以改用 Grep 测试
        let view = result("Grep", "intro\n---DIFF---\nold\n---DIFF---\nnew");
        let block = render_tool_result("t1-result", &view, &RenderCtx { text_width: 80 });

        assert!(
            block.lines.iter().any(|l| l.plain.contains("---DIFF---")),
            "非 Edit 工具应保留 ---DIFF--- 原文（不渲染为 diff），got: {:?}",
            block
                .lines
                .iter()
                .map(|l| l.plain.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_render_tool_result_plain_keeps_fence_markers_as_text_in_dim() {
        // Plain 纯文本原样（#91）：result 里的 ``` fence、code、after 全作普通文本保留，
        // 不做 markdown 重渲染（无 CODE 色），整体用暗色 TEXT_DIM（不跟随状态绿/红）。
        let view = result("Bash", "```\ncode\n```\nafter");
        let block = render_tool_result("t1-result", &view, &RenderCtx { text_width: 80 });

        assert!(
            block.lines.iter().any(|l| l.plain.contains("```")),
            "fence 标记应作普通文本原样保留"
        );
        assert!(block.lines.iter().any(|l| l.plain == "code"));
        assert!(block.lines.iter().any(|l| l.plain == "after"));
        // 内容预览行用暗色
        assert!(
            block
                .lines
                .iter()
                .all(|l| l.spans.iter().all(|s| s.style.fg == Some(theme::TEXT_DIM))),
            "Plain 预览内容行用暗色 TEXT_DIM"
        );
    }

    #[test]
    fn test_render_tool_result_plain_unclosed_fence_does_not_panic() {
        // 边界：纯文本预览对无闭合 fence 不 panic，原样逐行保留。
        let view = result("Bash", "```\nline1\nline2");

        let block = render_tool_result("t1-result", &view, &RenderCtx { text_width: 80 });

        assert!(block.lines.iter().any(|l| l.plain == "line1"));
        assert!(block.lines.iter().any(|l| l.plain == "line2"));
    }

    #[test]
    fn test_render_tool_result_hidden_renders_empty() {
        // Read 的 result 策略是 Hidden，应渲染空。
        let view = result("Read", "file content");

        let block = render_tool_result("t1-result", &view, &RenderCtx { text_width: 80 });

        assert!(block.lines.is_empty(), "Hidden 策略应渲染空");
    }

    #[test]
    fn test_render_tool_result_worktree_tools_do_not_truncate_fixed_context_result() {
        // #75：EnterWorktree/ExitWorktree 的结果行数固定且较少，应完整展示，不出现 omitted。
        let result_text = "已进入 worktree：branch feature/75\n当前分支：feature/75\n当前 path_base：/repo/.worktrees/feature-75\n当前 workspace_root：/repo/.worktrees/feature-75\n\n后续 Read/Edit/Write/Glob/Grep/Bash 请优先使用相对路径。\n如果必须使用绝对路径，必须位于当前 workspace_root 下。\n不要继续使用进入 worktree 前的 checkout/main workspace 绝对路径。";

        for tool_title in ["EnterWorktree", "ExitWorktree"] {
            let view = result(tool_title, result_text);
            let block = render_tool_result("worktree-result", &view, &RenderCtx { text_width: 80 });

            assert!(
                block
                    .lines
                    .iter()
                    .any(|line| line.plain.contains("不要继续使用进入 worktree 前")),
                "{tool_title} 应完整展示固定 worktree 上下文结果，实际: {:?}",
                block
                    .lines
                    .iter()
                    .map(|line| line.plain.as_str())
                    .collect::<Vec<_>>()
            );
            assert!(
                block
                    .lines
                    .iter()
                    .all(|line| !line.plain.contains("lines omitted")),
                "{tool_title} 不应显示 omitted 截断提示，实际: {:?}",
                block
                    .lines
                    .iter()
                    .map(|line| line.plain.as_str())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn test_render_tool_result_omitted_line_count_is_bounded() {
        // Bash 使用 tail 模式，不会显示 omitted，改用 Grep 测试
        let result_text = "line\n".repeat(OMITTED_LINE_COUNT_LIMIT + 20);
        let view = result("Grep", &result_text);

        let block = render_tool_result("t1-result", &view, &RenderCtx { text_width: 80 });

        assert!(block
            .lines
            .iter()
            .any(|line| line.plain.contains("10000+ lines omitted")));
    }

    #[test]
    fn test_render_tool_result_edit_diff_renders_with_numbers_signs_indent_color() {
        // #61 端到端：Edit 结果含 ---DIFF--- 标记，应渲染为带行号 + 加减语义色 +
        // 缩进 + 语法高亮的 diff 行，而非原始标记纯文本；ext 从 summary 推断。
        let view = result(
            "Edit",
            "replaced 1 occurrence(s) in src/lib.rs\n---DIFF---\nlet a = 1;\n---DIFF---\nlet a = 2;",
        );

        let block = render_tool_result("t1-result", &view, &RenderCtx { text_width: 80 });

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

    #[test]
    fn test_render_tool_result_tail_mode_shows_last_lines() {
        // tail 模式：只显示最后 N 行
        let result_text = "line1\nline2\nline3\nline4\nline5\nline6\nline7";
        let view = result("Bash", result_text);

        let block = render_tool_result("t1-result", &view, &RenderCtx { text_width: 80 });

        // Bash 默认 5 行 tail 模式
        assert!(block
            .lines
            .iter()
            .any(|l| l.plain.contains("... (2 lines above)")));
        assert!(block.lines.iter().any(|l| l.plain == "line3"));
        assert!(block.lines.iter().any(|l| l.plain == "line4"));
        assert!(block.lines.iter().any(|l| l.plain == "line5"));
        assert!(block.lines.iter().any(|l| l.plain == "line6"));
        assert!(block.lines.iter().any(|l| l.plain == "line7"));
        // 前面的行不应出现
        assert!(!block.lines.iter().any(|l| l.plain == "line1"));
        assert!(!block.lines.iter().any(|l| l.plain == "line2"));
    }
}

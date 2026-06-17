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
    let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

    assert_eq!(block.block_id, "t1-result");
    assert!(block
        .lines
        .iter()
        .any(|line| line.plain.contains("done: 3 matches")));
}

#[test]
fn test_render_tool_result_plain_wraps_long_lines_to_render_width() {
    let view = result("Bash", "abcdef");
    let block = render_tool_result("t1-result", &view, &RenderCtx { width: 4 });

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
    let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

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
    let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

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

    let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

    assert!(block.lines.iter().any(|l| l.plain == "line1"));
    assert!(block.lines.iter().any(|l| l.plain == "line2"));
}

#[test]
fn test_render_tool_result_hidden_renders_empty() {
    // Read 的 result 策略是 Hidden，应渲染空。
    let view = result("Read", "file content");

    let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

    assert!(block.lines.is_empty(), "Hidden 策略应渲染空");
}

#[test]
fn test_render_tool_result_worktree_tools_do_not_truncate_fixed_context_result() {
    // #75：EnterWorktree/ExitWorktree 的结果行数固定且较少，应完整展示，不出现 omitted。
    let result_text = "已进入 worktree：branch feature/75\n当前分支：feature/75\n当前 path_base：/repo/.worktrees/feature-75\n当前 working_root：/repo/.worktrees/feature-75\n\n后续 Read/Edit/Write/Glob/Grep/Bash 请优先使用相对路径。\n如果必须使用绝对路径，必须位于当前 working_root 下。\n不要继续使用进入 worktree 前的 checkout/main workspace 绝对路径。";

    for tool_title in ["EnterWorktree", "ExitWorktree"] {
        let view = result(tool_title, result_text);
        let block = render_tool_result("worktree-result", &view, &RenderCtx { width: 80 });

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

    let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

    assert!(block
        .lines
        .iter()
        .any(|line| line.plain.contains("10000+ lines omitted")));
}

#[test]
fn test_render_tool_result_edit_diff_renders_with_numbers_signs_indent_color() {
    // #61 端到端：Edit 结果含 ---DIFF--- 标记，应渲染为带行号 + 加减语义色 +
    // 缩进 + 语法高亮的 diff 行，而非原始标记纯文本；ext 从 summary 推断。
    let mut view = result(
        "Edit",
        "replaced 1 occurrence(s) in src/lib.rs\n---DIFF---\nlet a = 1;\n---DIFF---\nlet a = 2;",
    );

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

#[test]
fn test_render_tool_result_tail_mode_shows_last_lines() {
    // tail 模式：只显示最后 N 行
    let result_text = "line1\nline2\nline3\nline4\nline5\nline6\nline7";
    let view = result("Bash", result_text);

    let block = render_tool_result("t1-result", &view, &RenderCtx { width: 80 });

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

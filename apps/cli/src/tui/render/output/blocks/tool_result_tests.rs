use super::*;
use crate::tui::render::theme;
use crate::tui::view_model::output::{
    AgentActivityKindView, AgentActivityLineView, ToolResultBlockView,
};

use crate::tui::view_model::style::SemanticStyle;

fn result(tool_title: &str, result_text: &str) -> ToolResultBlockView {
    ToolResultBlockView {
        key: format!("{tool_title}-result"),
        tool_title: tool_title.into(),
        args_preview: None,
        result_text: result_text.into(),
        activity_lines: None,
        workspace_root: None,
        data: None,
        style: SemanticStyle::Success,
    }
}

fn result_with_data(
    tool_title: &str,
    result_text: &str,
    data: serde_json::Value,
) -> ToolResultBlockView {
    ToolResultBlockView {
        key: format!("{tool_title}-result"),
        tool_title: tool_title.into(),
        args_preview: None,
        result_text: result_text.into(),
        activity_lines: None,
        workspace_root: None,
        data: Some(data),
        style: SemanticStyle::Success,
    }
}

#[test]
fn test_render_tool_result_uses_typed_activity_kind_for_arrow() {
    let mut view = result("Agent", "Read src/lib.rs\nRead as prose");
    view.activity_lines = Some(vec![
        AgentActivityLineView {
            kind: AgentActivityKindView::ToolCall,
            content: crate::tui::view_model::output::AgentActivityContentView::ToolCall {
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "src/lib.rs"}),
            },
        },
        AgentActivityLineView {
            kind: AgentActivityKindView::Message,
            content: "Read as prose".into(),
        },
    ]);

    let block = render_tool_result("agent-streaming-result", &view, &RenderCtx::for_width(80));
    let rendered: Vec<_> = block.lines.iter().map(|line| line.plain.as_str()).collect();

    assert_eq!(rendered, vec!["→ Read src/lib.rs", "Read as prose"]);
    assert!(rendered.iter().all(|line| !line.contains("→ →")));
    assert!(rendered.iter().all(|line| !line.contains('⎿')));
}

#[test]
fn sub_run_tool_call_header_uses_workspace_root_at_render_boundary() {
    let mut view = result("Agent", "Read /repo/src/lib.rs");
    view.workspace_root = Some(std::path::PathBuf::from("/repo"));
    view.activity_lines = Some(vec![AgentActivityLineView {
        kind: AgentActivityKindView::ToolCall,
        content: crate::tui::view_model::output::AgentActivityContentView::ToolCall {
            name: "Read".to_string(),
            input: serde_json::json!({"file_path": "/repo/src/lib.rs"}),
        },
    }]);

    let block = render_tool_result("agent-streaming-result", &view, &RenderCtx::for_width(80));

    assert_eq!(block.lines[0].plain, "→ Read src/lib.rs");
    assert!(!block.lines[0].plain.contains("/repo/"));
}

#[test]
fn test_render_tool_result_renders_result_text_lines() {
    // 正常路径：result_text 应作为结果行渲染。
    let view = result("Grep", "done: 3 matches");
    let block = render_tool_result("t1-result", &view, &RenderCtx::for_width(80));

    assert_eq!(block.block_id, "t1-result");
    assert!(block
        .lines
        .iter()
        .any(|line| line.plain.contains("done: 3 matches")));
}

#[test]
fn test_render_tool_result_plain_wraps_long_lines_to_render_width() {
    let view = result("Bash", "abcdef");
    let block = render_tool_result("t1-result", &view, &RenderCtx::for_width(4));

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
    let block = render_tool_result("t1-result", &view, &RenderCtx::for_width(80));

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
    let block = render_tool_result("t1-result", &view, &RenderCtx::for_width(80));

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

    let block = render_tool_result("t1-result", &view, &RenderCtx::for_width(80));

    assert!(block.lines.iter().any(|l| l.plain == "line1"));
    assert!(block.lines.iter().any(|l| l.plain == "line2"));
}

#[test]
fn test_render_tool_result_hidden_renders_empty() {
    // Read 的 result 策略是 Hidden，应渲染空。
    let view = result("Read", "file content");

    let block = render_tool_result("t1-result", &view, &RenderCtx::for_width(80));

    assert!(block.lines.is_empty(), "Hidden 策略应渲染空");
}

#[test]
fn test_render_tool_result_worktree_tools_do_not_truncate_fixed_context_result() {
    // #75：EnterWorktree/ExitWorktree 的结果行数固定且较少，应完整展示，不出现 omitted。
    let result_text = "已进入 worktree：branch feature/75\n当前分支：feature/75\n当前 path_base：/repo/.worktrees/feature-75\n当前 workspace_root：/repo/.worktrees/feature-75\n\n后续 Read/Edit/Write/Glob/Grep/Bash 请优先使用相对路径。\n如果必须使用绝对路径，必须位于当前 workspace_root 下。\n不要继续使用进入 worktree 前的 checkout/main workspace 绝对路径。";

    for tool_title in ["EnterWorktree", "ExitWorktree"] {
        let view = result(tool_title, result_text);
        let block = render_tool_result("worktree-result", &view, &RenderCtx::for_width(80));

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

    let block = render_tool_result("t1-result", &view, &RenderCtx::for_width(80));

    assert!(block
        .lines
        .iter()
        .any(|line| line.plain.contains("10000+ lines omitted")));
}

#[test]
fn test_render_tool_result_edit_diff_renders_from_structured_data() {
    // #546：Edit diff 通过结构化 data 通道（EditResult JSON）驱动渲染，
    // 不再依赖 text 中的 ---DIFF--- 标记。
    let data = serde_json::json!({
        "file_path": "src/lib.rs",
        "replacements_made": 1,
        "dry_run": false,
        "old": "let a = 1;",
        "new": "let a = 2;",
        "start_line": 1
    });
    let view = result_with_data("Edit", "Replaced 1 occurrence(s) in src/lib.rs", data);

    let block = render_tool_result("t1-result", &view, &RenderCtx::for_width(80));

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
}

#[test]
fn test_render_tool_result_edit_diff_falls_back_to_text_for_legacy_sessions() {
    // 历史兼容：旧 session 的 data 没有 old/new/start_line，回退到 parse_edit_diff。
    let view = result(
        "Edit",
        "replaced 1 occurrence(s) in src/lib.rs\n---DIFF---\nlet a = 1;\n---DIFF---\nlet a = 2;",
    );

    let block = render_tool_result("t1-result", &view, &RenderCtx::for_width(80));

    assert!(
        block.lines.iter().any(|line| line.plain.contains("1;")),
        "回退路径也应正确渲染 diff"
    );
}

#[test]
fn test_render_tool_result_tail_mode_shows_last_lines() {
    // tail 模式：只显示最后 N 行
    let result_text = "line1\nline2\nline3\nline4\nline5\nline6\nline7";
    let view = result("Bash", result_text);

    let block = render_tool_result("t1-result", &view, &RenderCtx::for_width(80));

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

// ── #1895 L4：typed 渲染接线全链（真实 BlockView → 渲染 → 行断言） ──
// 捕获两类中间层断裂：lookup_display 名字不匹配、content(data) 传递
// 断链——两者都会静默回退 fallback JSON 路径，仅测 Display 方法本身
// 无法发现。

#[test]
fn background_process_logs_result_renders_parsed_lines_via_render_pipeline() {
    let logs_json = serde_json::json!({
        "log": {
            "text": "64 bytes from 127.0.0.1: icmp_seq=1\n64 bytes from 127.0.0.1: icmp_seq=2",
            "cursor": 120,
            "total_written": 120,
        }
    });
    let view = result_with_data("BackgroundProcessLogs", "{\"raw\":true}", logs_json);
    let block = render_tool_result("bgp-logs-result", &view, &RenderCtx::for_width(80));
    let text = block_text(&block);
    assert!(
        text.contains("icmp_seq=1"),
        "Logs 应经 typed 分支渲染多行原文：{text}"
    );
    assert!(
        !text.contains("\\\"raw\\\""),
        "typed 分支生效时不应回退 JSON 原文：{text}"
    );
}

#[test]
fn background_process_list_result_renders_summary_via_render_pipeline() {
    let list_json = serde_json::json!({
        "tasks": [
            {
                "task_id": "bgp_07YExample02",
                "tool_name": "Bash",
                "state": "backgrounded",
                "summary": "ping -c 30 127.0.0.1",
            }
        ]
    });
    let view = result_with_data("BackgroundProcessList", "[]", list_json);
    let block = render_tool_result("bgp-list-result", &view, &RenderCtx::for_width(80));
    let text = block_text(&block);
    assert!(
        text.contains("ping -c 30"),
        "List 应渲染逐行摘要（summary 来自 data 而非 result_text）：{text}"
    );
    assert!(
        !text.contains("\"processes\":"),
        "不应显示未解析的 JSON 原文：{text}"
    );
}

fn block_text(block: &super::RenderedBlock) -> String {
    block
        .lines
        .iter()
        .map(|line| line.plain.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

use super::*;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::view_model::output::{
    OutputBlockKind, TextBlockView, ToolCallBlockView, ToolResultBlockView, ToolSemanticStatus,
};
use crate::tui::view_model::style::SemanticStyle;
use ratatui::text::Span;

fn tool(status: ToolSemanticStatus) -> OutputBlockKind {
    OutputBlockKind::ToolCall(ToolCallBlockView {
        key: "t".into(),
        chat_id: None,
        run_id: None,
        tool_call_id: None,
        title: "Grep".into(),
        icon: "●".into(),
        semantic_status: status,
        style: SemanticStyle::Running,
        args_preview: None,
        streaming_preview: None,
        result_summary: None,
        result_payload: None,
        workspace_root: None,
        collapsible: false,
        collapsed: false,
        agent_meta: None,
    })
}

#[test]
fn test_marker_glyph_for_tool_status() {
    assert_eq!(marker_glyph(&tool(ToolSemanticStatus::Pending)), "○");
    assert_eq!(marker_glyph(&tool(ToolSemanticStatus::Success)), "✓");
    assert_eq!(marker_glyph(&tool(ToolSemanticStatus::Error)), "✗");
    assert_eq!(marker_glyph(&tool(ToolSemanticStatus::Cancelled)), "✗");
    assert_eq!(marker_glyph(&tool(ToolSemanticStatus::Running)), "●");
}

#[test]
fn test_marker_glyph_for_assistant_message_is_filled_circle() {
    let kind = OutputBlockKind::AssistantMessage(TextBlockView {
        key: "a".into(),
        text: "answer".into(),
        style: SemanticStyle::Normal,
    });

    assert_eq!(marker_glyph(&kind), "●");
    assert_eq!(marker_color(&kind), theme::ASSISTANT);
}

#[test]
fn test_marker_glyph_for_tool_result_is_corner() {
    let kind = OutputBlockKind::ToolResult(ToolResultBlockView {
        key: "r".into(),
        tool_title: "Bash".into(),
        args_preview: None,
        result_text: "done".into(),
        activity_lines: None,
        workspace_root: None,
        data: None,
        style: SemanticStyle::Success,
    });

    assert_eq!(marker_glyph(&kind), "⎿");
    assert_eq!(marker_color(&kind), theme::TEXT_MUTED);
}

#[test]
fn test_animated_marker_glyph_blinks_running_tool_between_filled_and_open_circle() {
    let running = tool(ToolSemanticStatus::Running);
    assert_eq!(animated_marker_glyph(&running, 0), "●");
    assert_eq!(animated_marker_glyph(&running, 1), "●");
    assert_eq!(animated_marker_glyph(&running, 3), "●");
    assert_eq!(animated_marker_glyph(&running, 4), "○");
    assert_eq!(animated_marker_glyph(&running, 7), "○");
    assert_eq!(animated_marker_glyph(&running, 8), "●");
}

#[test]
fn test_animated_marker_glyph_blinks_running_tool_with_same_divisor() {
    let running = tool(ToolSemanticStatus::Running);
    assert_eq!(animated_marker_glyph(&running, 0), "●");
    assert_eq!(animated_marker_glyph(&running, 4), "○");
    assert_eq!(animated_marker_glyph(&running, 8), "●");
}

#[test]
fn test_animated_marker_glyph_keeps_finished_tool_static() {
    let success = tool(ToolSemanticStatus::Success);
    assert_eq!(animated_marker_glyph(&success, 0), "✓");
    assert_eq!(animated_marker_glyph(&success, 1), "✓");
}

#[test]
fn test_apply_gutter_first_line_has_marker_rest_blank_not_in_plain() {
    let kind = tool(ToolSemanticStatus::Success);
    let lines = vec![
        RenderedLine::new(vec![Span::raw("Grep /x/")]),
        RenderedLine::new(vec![Span::raw("detail")]),
    ];
    let out = apply_gutter(&kind, 0, lines);
    assert!(out[0].spans[0].content.as_ref().contains('✓'));
    assert_eq!(out[0].plain, "Grep /x/");
    assert!(out[1].spans[0].content.as_ref().chars().all(|c| c == ' '));
    assert_eq!(out[1].plain, "detail");
}

#[test]
fn test_apply_gutter_depth_widens_indent() {
    let kind = OutputBlockKind::SystemNotice(TextBlockView {
        key: "s".into(),
        text: "x".into(),
        style: SemanticStyle::Muted,
    });
    let d0 = apply_gutter(&kind, 0, vec![RenderedLine::new(vec![Span::raw("x")])]);
    let d1 = apply_gutter(&kind, 1, vec![RenderedLine::new(vec![Span::raw("x")])]);
    let w0 = d0[0].spans[0].content.chars().count();
    let w1 = d1[0].spans[0].content.chars().count();
    assert!(w1 > w0, "depth 越深，gutter 前导越宽");
    assert_eq!(d1[0].plain, "x", "缩进不进 plain");
}

#[test]
fn test_apply_gutter_sets_gutter_cols() {
    let kind = tool(ToolSemanticStatus::Success);
    let lines = vec![
        RenderedLine::new(vec![Span::raw("Grep")]),
        RenderedLine::new(vec![Span::raw("detail")]),
    ];
    let d0 = apply_gutter(&kind, 0, lines.clone());
    assert_eq!(d0[0].gutter_cols, gutter_width(0));
    assert_eq!(
        d0[1].gutter_cols,
        gutter_width(0),
        "续行 gutter_cols 同首行"
    );
    // gutter_cols 须等于首 span 字符数（不变式：均宽度 1 字符）。
    assert_eq!(d0[0].spans[0].content.chars().count(), d0[0].gutter_cols);

    let d1 = apply_gutter(&kind, 1, lines);
    assert_eq!(d1[0].gutter_cols, gutter_width(1));
    assert_eq!(d1[0].spans[0].content.chars().count(), d1[0].gutter_cols);
}

#[test]
fn test_apply_gutter_wide_marker_fills_slot_chars_not_display_width() {
    // 💭（宽字符 2 列）作 ThinkingMessage marker：占满 2 列 marker 槽、无尾空格；
    // 内容与窄 marker block 同列对齐；gutter_cols = 实际字符数（1，非显示列 2）。
    let kind = OutputBlockKind::ThinkingMessage(TextBlockView {
        key: "t".into(),
        text: "x".into(),
        style: SemanticStyle::Muted,
    });
    let out = apply_gutter(
        &kind,
        0,
        vec![
            RenderedLine::new(vec![Span::raw("ponder")]),
            RenderedLine::new(vec![Span::raw("more")]),
        ],
    );

    assert_eq!(
        out[0].spans[0].content.as_ref(),
        "💭",
        "首行 marker = 💭，无尾空格"
    );
    assert_eq!(
        out[0].gutter_cols, 1,
        "gutter_cols = 字符数（💭 1 字符），非显示列 2"
    );
    assert_eq!(out[1].spans[0].content.as_ref(), "  ", "续行等宽空白 2 列");
    assert_eq!(out[1].gutter_cols, 2);
}

// ─── effective_block_width 单测（#329 根因契约）───

#[test]
fn test_effective_block_width_subtracts_depth_zero_gutter() {
    // depth=0 gutter=2：outer=80 → 78（80-2）
    assert_eq!(effective_block_width(80, 0), 78);
    assert_eq!(effective_block_width(77, 0), 75);
}

#[test]
fn test_effective_block_width_subtracts_depth_one_gutter() {
    // depth=1 gutter=4（2 + PER_DEPTH_INDENT=2）：outer=80 → 76
    assert_eq!(effective_block_width(80, 1), 76);
    assert_eq!(effective_block_width(77, 1), 73);
}

#[test]
fn test_effective_block_width_subtracts_depth_two_gutter() {
    // depth=2 gutter=6：outer=80 → 74
    assert_eq!(effective_block_width(80, 2), 74);
}

#[test]
fn test_effective_block_width_saturates_when_outer_equals_gutter() {
    // 极窄屏（<30）gutter 完全移除 → effective == outer
    assert_eq!(effective_block_width(2, 0), 2);
    assert_eq!(effective_block_width(4, 1), 4);
}

#[test]
fn test_effective_block_width_saturates_when_outer_less_than_gutter() {
    // 极窄屏（<30）gutter 移除 → effective == outer
    assert_eq!(effective_block_width(1, 0), 1);
    assert_eq!(effective_block_width(0, 0), 0);
    // 正常屏：depth 100 gutter > 100 → 0
    assert_eq!(effective_block_width(100, 100), 0);
}

#[test]
fn test_effective_block_width_handles_huge_depth_without_overflow() {
    // 错误路径：usize::MAX depth 不应 panic，u16::try_from 失败时用 u16::MAX 兜底 → 0
    assert_eq!(effective_block_width(80, usize::MAX), 0);
    // depth 70 gutter=142 > 80 → 0（正常屏缩进=2*70=140+2=142）
    assert_eq!(effective_block_width(80, 70), 0);
}

#[test]
fn test_effective_block_width_plus_gutter_round_trip_equals_outer_when_within_budget() {
    // 不变式：effective + gutter == outer（前提：outer ≥ gutter 且在正常屏模式下）
    // 只测 outer ≥ 50（正常屏，缩进=PER_DEPTH_INDENT），避免窄屏逻辑干扰。
    for outer in [50u16, 77, 120, 200] {
        for depth in [0usize, 1, 2, 3, 5] {
            let gw = gutter_width(depth) as u16;
            if outer < gw {
                continue;
            }
            let eff = effective_block_width(outer, depth);
            assert_eq!(
                eff + gw,
                outer,
                "outer={} depth={}：effective({}) + gutter({}) 应 == outer",
                outer,
                depth,
                eff,
                gw
            );
        }
    }
}

#[test]
fn test_apply_gutter_with_frame_caps_huge_depth_without_panic_or_oom() {
    // #329 防御：`apply_gutter_with_frame` 内部 `" ".repeat(indent_n)`，
    // 若 depth 无上限，`usize::MAX` 会触发 OOM panic。本测试用 usize::MAX
    // 验证 cap 到 MAX_GUTTER_DEPTH（256）后不 panic、输出仍合法。
    let line = RenderedLine::from_plain("hello");
    let lines = vec![line.clone(), line];
    let view = TextBlockView {
        key: String::new(),
        text: String::new(),
        style: SemanticStyle::Normal,
    };
    let out = apply_gutter_with_frame(&OutputBlockKind::UserMessage(view), usize::MAX, lines, 0);
    assert_eq!(out.len(), 2, "lines 数应保持");
    // indent 被 cap 到 MAX_GUTTER_DEPTH * PER_DEPTH_INDENT = 512 列
    // + marker 槽 2 列 = 514 列总 gutter_cols。不可能 panic。
    assert!(out[0].gutter_cols > GUTTER_WIDTH);
}

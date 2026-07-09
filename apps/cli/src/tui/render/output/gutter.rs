//! 行首标志槽 gutter：depth 缩进 + marker 列。组合期注入，只进 spans 不进 plain。
//! marker 按 kind/status 决定；运行态工具 marker 可随动画帧闪烁，仅首行画，后续行等宽空白。

use crate::tui::render::display::safe_text::str_display_width;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::theme;
use crate::tui::view_model::output::{HookNoticeSemanticKind, OutputBlockKind, ToolSemanticStatus};
use ratatui::style::Style;
use ratatui::text::Span;

/// marker 列字符宽度（字形 1 + 空格 1）。
pub const GUTTER_WIDTH: usize = 2;
const PER_DEPTH_INDENT: usize = 2;
pub const TOOL_MARKER_BLINK_DIVISOR: u64 = 4;
/// depth 上限防御（防 `" ".repeat()` 爆内存）。实际对话树深度通常 ≤ 4
/// （root + tool result 子块），256 已是巨幅冗余，仅用于 fuzz / 错误输入。
const MAX_GUTTER_DEPTH: usize = 256;

/// 按 block 类型 / 工具状态映射 marker 字形。多数为单列字形，宽字符（如 💭）由
/// `apply_gutter` 按显示宽度补白填满 marker 槽。
pub fn marker_glyph(kind: &OutputBlockKind) -> &'static str {
    animated_marker_glyph(kind, 0)
}

/// 按 block 类型 / 工具状态和动画帧映射 marker 字形。
pub fn animated_marker_glyph(kind: &OutputBlockKind, animation_frame: u64) -> &'static str {
    match kind {
        OutputBlockKind::ToolCall(t) => match t.semantic_status {
            ToolSemanticStatus::Pending => "○",
            ToolSemanticStatus::Success => "✓",
            ToolSemanticStatus::Error => "✗",
            ToolSemanticStatus::Cancelled => "–",
            ToolSemanticStatus::Orphaned => "?",
            ToolSemanticStatus::Running => {
                let blink_frame = animation_frame / TOOL_MARKER_BLINK_DIVISOR;
                if blink_frame.is_multiple_of(2) {
                    "●"
                } else {
                    "○"
                }
            }
        },
        OutputBlockKind::UserMessage(_) => ">",
        OutputBlockKind::AssistantMessage(_) => "●",
        // 💭 顶格作 thinking marker（宽字符占满 2 列 marker 槽，无尾空格）。
        OutputBlockKind::ThinkingMessage(_) | OutputBlockKind::ModelStreamPlaceholder(_) => "💭",
        // ⎿ 圆角连接到父 ToolCall header，表示这是工具结果子块。
        OutputBlockKind::ToolResult(_) => "⎿",
        OutputBlockKind::HookNotice(h) => match h.kind {
            HookNoticeSemanticKind::Blocked | HookNoticeSemanticKind::Failed => "⊘",
            HookNoticeSemanticKind::Info => "ℹ",
        },
        _ => " ",
    }
}

/// marker 字形的前景色（按 block 类型 / 工具状态）。
fn marker_color(kind: &OutputBlockKind) -> ratatui::style::Color {
    match kind {
        OutputBlockKind::ToolCall(t) => match t.semantic_status {
            ToolSemanticStatus::Pending => theme::TEXT_MUTED,
            ToolSemanticStatus::Success => theme::SUCCESS,
            ToolSemanticStatus::Error => theme::ERROR,
            ToolSemanticStatus::Running => theme::TOOL_RUNNING,
            ToolSemanticStatus::Cancelled => theme::TEXT_MUTED,
            ToolSemanticStatus::Orphaned => theme::WARNING,
        },
        OutputBlockKind::UserMessage(_) => theme::USER,
        OutputBlockKind::AssistantMessage(_) => theme::ASSISTANT,
        OutputBlockKind::ThinkingMessage(_) | OutputBlockKind::ModelStreamPlaceholder(_) => {
            theme::THINKING
        }
        OutputBlockKind::ToolResult(_) => theme::TEXT_MUTED,
        OutputBlockKind::HookNotice(h) => match h.kind {
            HookNoticeSemanticKind::Blocked | HookNoticeSemanticKind::Failed => theme::ERROR,
            HookNoticeSemanticKind::Info => theme::TEXT_MUTED,
        },
        _ => theme::TEXT_MUTED,
    }
}

/// gutter 总显示宽度（供选区列偏移补偿用）。
///
/// 任意 `usize` depth 都安全：saturating 运算保证不溢出（防御性 depth 来自
/// `effective_block_width` 的错误路径测试）。
pub fn gutter_width(depth: usize) -> usize {
    depth
        .saturating_mul(PER_DEPTH_INDENT)
        .saturating_add(GUTTER_WIDTH)
}

/// block 文本可用宽度 = `outer_width - gutter_width(depth)`。
///
/// 调用方应在把宽度塞进 `RenderCtx.text_width` 之前用本函数扣除组合期注入的
/// gutter，保证 wrap 后的 line 加回 gutter 后总可见宽 ≤ `outer_width`
/// （即 `content_area.width`，Paragraph 渲染宽度）。
///
/// **根因契约（issue #329）**：document 预 wrap 宽度未扣 gutter，导致
/// `Paragraph::new` 默认 LineTruncator 把行尾字符吞掉。本函数就是修正入口。
///
/// 边界：outer 不够时 `saturating_sub` 保证返回非负；`outer=0` 时返回 0，
/// 让上层 wrap 路径走 `max_width=0` 短路分支（见 `wrap_spans_to_rendered_lines`）。
pub fn effective_block_width(outer_width: u16, depth: usize) -> u16 {
    let gw = u16::try_from(gutter_width(depth)).unwrap_or(u16::MAX);
    outer_width.saturating_sub(gw)
}

/// 为一个 block 的所有行前置 gutter（首行带 marker，余行等宽空白）。gutter 只进 spans，不进 plain。
pub fn apply_gutter(
    kind: &OutputBlockKind,
    depth: usize,
    lines: Vec<RenderedLine>,
) -> Vec<RenderedLine> {
    apply_gutter_with_frame(kind, depth, lines, 0)
}

/// 为一个 block 的所有行前置带动画帧的 gutter。仅运行态工具 marker 消费动画帧。
pub fn apply_gutter_with_frame(
    kind: &OutputBlockKind,
    depth: usize,
    lines: Vec<RenderedLine>,
    animation_frame: u64,
) -> Vec<RenderedLine> {
    let glyph = animated_marker_glyph(kind, animation_frame);
    let color = marker_color(kind);
    // cap depth 防 `" ".repeat()` 爆内存（`gutter_width` 路径已 saturating，
    // 但 repeat 仍会按 saturating 后的 usize 分配，可能 OOM）。
    let indent_n = depth.min(MAX_GUTTER_DEPTH).saturating_mul(PER_DEPTH_INDENT);
    lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let gutter_text = if i == 0 {
                // marker 槽总显示宽 GUTTER_WIDTH：窄字形（✓/>）补 1 尾空格，
                // 宽字符（💭，2 列）补 0——按显示宽度补白，保证续行等宽对齐。
                let pad = GUTTER_WIDTH.saturating_sub(str_display_width(glyph));
                format!("{}{glyph}{}", " ".repeat(indent_n), " ".repeat(pad))
            } else {
                " ".repeat(indent_n.saturating_add(GUTTER_WIDTH))
            };
            // gutter_cols = gutter span 实际字符数（选区按字符跳过 gutter）：窄 marker 行
            // 字符数 == 显示列数 == gutter_width(depth)；宽字符 marker（💭）字符数更少，但其
            // 显示宽仍占满 marker 槽，续行等宽对齐与内容起列不受影响。
            let gutter_cols = gutter_text.chars().count();
            let mut spans = vec![Span::styled(gutter_text, Style::default().fg(color))];
            spans.extend(line.spans);
            let mut gutted = RenderedLine::with_plain(spans, line.plain);
            gutted.style = line.style;
            gutted.gutter_cols = gutter_cols;
            gutted.fill_style = line.fill_style;
            gutted
        })
        .collect()
}

#[cfg(test)]
mod tests {
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
            turn_id: None,
            tool_call_id: None,
            title: "Grep".into(),
            icon: "●".into(),
            semantic_status: status,
            style: SemanticStyle::Running,
            args_preview: None,
            activity_lines: Vec::new(),
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
        // 边界：outer 刚好等于 gutter（depth=0）→ 0（让 wrap 走短路分支）
        assert_eq!(effective_block_width(2, 0), 0);
        assert_eq!(effective_block_width(4, 1), 0);
    }

    #[test]
    fn test_effective_block_width_saturates_when_outer_less_than_gutter() {
        // 边界：outer < gutter → 0（saturating_sub），不允许 wrap 占用 gutter 列
        assert_eq!(effective_block_width(1, 0), 0);
        assert_eq!(effective_block_width(0, 0), 0);
        assert_eq!(effective_block_width(2, 1), 0);
        assert_eq!(effective_block_width(100, 100), 0);
    }

    #[test]
    fn test_effective_block_width_handles_huge_depth_without_overflow() {
        // 错误路径：usize::MAX depth 不应 panic，u16::try_from 失败时用 u16::MAX 兜底 → 0
        assert_eq!(effective_block_width(80, usize::MAX), 0);
        assert_eq!(
            effective_block_width(80, 70),
            0,
            "depth 70 gutter=142 > 80 → 0"
        );
    }

    #[test]
    fn test_effective_block_width_plus_gutter_round_trip_equals_outer_when_within_budget() {
        // 不变式：effective + gutter == outer（前提：outer ≥ gutter）
        for outer in [10u16, 20, 50, 77, 120, 200] {
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
        let out =
            apply_gutter_with_frame(&OutputBlockKind::UserMessage(view), usize::MAX, lines, 0);
        assert_eq!(out.len(), 2, "lines 数应保持");
        // indent 被 cap 到 MAX_GUTTER_DEPTH * PER_DEPTH_INDENT = 512 列
        // + marker 槽 2 列 = 514 列总 gutter_cols。不可能 panic。
        assert!(out[0].gutter_cols > GUTTER_WIDTH);
    }
}

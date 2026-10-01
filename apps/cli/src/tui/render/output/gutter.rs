//! 行首标志槽 gutter：depth 缩进 + marker 列。组合期注入，只进 spans 不进 plain。
//! marker 按 kind/status 决定；运行态工具 marker 可随动画帧闪烁，仅首行画，后续行等宽空白。

use super::constants::{
    GUTTER_WIDTH, MAX_GUTTER_DEPTH, NARROW_NO_GUTTER_THRESHOLD, NARROW_NO_INDENT_THRESHOLD,
    PER_DEPTH_INDENT, TOOL_MARKER_BLINK_DIVISOR,
};
use crate::tui::render::display::safe_text::str_display_width;
use crate::tui::render::output::rendered::{LineAnimation, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::{OutputBlockKind, ToolSemanticStatus};
use ratatui::style::Style;
use ratatui::text::Span;

/// 窄屏模式下实际使用的 per-depth 缩进列数。
fn effective_per_depth_indent(outer_width: u16) -> usize {
    if outer_width < NARROW_NO_INDENT_THRESHOLD {
        0
    } else {
        PER_DEPTH_INDENT
    }
}

/// 窄屏模式下实际使用的 gutter 总宽度（含 marker + indent）。
fn effective_gutter_width(outer_width: u16, depth: usize) -> usize {
    if outer_width < NARROW_NO_GUTTER_THRESHOLD {
        0
    } else {
        gutter_width_with_indent(depth, effective_per_depth_indent(outer_width))
    }
}

/// 是否完全移除 gutter（极窄屏）。
pub fn is_gutter_suppressed(outer_width: u16) -> bool {
    outer_width < NARROW_NO_GUTTER_THRESHOLD
}

/// 按 block 类型 / 工具状态映射 marker 字形。多数为单列字形，宽字符（如 💭）由
/// `apply_gutter` 按显示宽度补白填满 marker 槽。
#[cfg(test)]
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
            ToolSemanticStatus::Cancelled => "✗",
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
        OutputBlockKind::ThinkingMessage(_) => "💭",
        // ⎿ 圆角连接到父 ToolCall header，表示这是工具结果子块。
        OutputBlockKind::ToolResult(_) => "⎿",
        OutputBlockKind::HookNotice(notice) => match notice.kind {
            crate::tui::adapter::runtime_view::TuiHookNoticeKind::Blocked
            | crate::tui::adapter::runtime_view::TuiHookNoticeKind::Failed => "⊘",
            crate::tui::adapter::runtime_view::TuiHookNoticeKind::Info => "ℹ",
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
            ToolSemanticStatus::Cancelled => theme::ERROR,
        },
        OutputBlockKind::UserMessage(_) => theme::USER,
        OutputBlockKind::AssistantMessage(_) => theme::ASSISTANT,
        OutputBlockKind::ThinkingMessage(_) => theme::THINKING,
        OutputBlockKind::ToolResult(_) => theme::TEXT_MUTED,
        OutputBlockKind::HookNotice(notice) => match notice.kind {
            crate::tui::adapter::runtime_view::TuiHookNoticeKind::Blocked
            | crate::tui::adapter::runtime_view::TuiHookNoticeKind::Failed => theme::ERROR,
            crate::tui::adapter::runtime_view::TuiHookNoticeKind::Info => theme::TEXT_MUTED,
        },
        _ => theme::TEXT_MUTED,
    }
}

/// gutter 总显示宽度（供选区列偏移补偿用）。
///
/// 任意 `usize` depth 都安全：saturating 运算保证不溢出（防御性 depth 来自
/// `effective_block_width` 的错误路径测试）。
#[cfg(test)]
pub fn gutter_width(depth: usize) -> usize {
    gutter_width_with_indent(depth, PER_DEPTH_INDENT)
}

/// 指定 per-depth 缩进的 gutter 总宽度。
fn gutter_width_with_indent(depth: usize, per_depth: usize) -> usize {
    depth.saturating_mul(per_depth).saturating_add(GUTTER_WIDTH)
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
    let gw = effective_gutter_width(outer_width, depth.min(MAX_GUTTER_DEPTH));
    outer_width.saturating_sub(u16::try_from(gw).unwrap_or(u16::MAX))
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
            gutted.links = line.links;
            gutted.animation = if i == 0
                && matches!(
                    kind,
                    OutputBlockKind::ToolCall(tool)
                        if tool.semantic_status == ToolSemanticStatus::Running
                ) {
                Some(LineAnimation::RunningToolMarker)
            } else {
                line.animation
            };
            gutted
        })
        .collect()
}

#[cfg(test)]
#[path = "gutter_tests.rs"]
mod tests;

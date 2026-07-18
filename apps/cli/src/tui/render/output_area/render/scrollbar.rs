use ratatui::layout::Rect;

/// 输出区右侧保留的呼吸空间列数（原 scrollbar 占位，现仅保留间距）。
pub(crate) const SCROLLBAR_RESERVE_COLS: u16 = 2;

/// 从 area 中扣除右侧呼吸空间。
/// `needs_scrollbar` 参数保留兼容但不再影响行为（scrollbar 已移除）。
pub(crate) fn content_area_for_scrollbar(area: Rect, _needs_scrollbar: bool) -> Rect {
    if area.width == 0 {
        return area;
    }
    Rect {
        x: area.x,
        y: area.y,
        width: area.width.saturating_sub(SCROLLBAR_RESERVE_COLS).max(1),
        height: area.height,
    }
}

/// 根据 `OutputViewState` 提供的滚动快照计算可见范围。
///
/// `should_auto_scroll` / `current_scroll_offset` 是调用方从 view_state 传入的只读投影，
/// 本模块不存储、不修改滚动真相，避免 render widget 重新持有 app/domain mirror 状态。
pub(super) fn visible_range(
    total_lines: usize,
    visible_lines: usize,
    should_auto_scroll: bool,
    current_scroll_offset: usize,
) -> (usize, usize) {
    if should_auto_scroll {
        let start = total_lines.saturating_sub(visible_lines);
        (start, total_lines)
    } else {
        let max_start = total_lines.saturating_sub(visible_lines);
        let start = max_start
            .saturating_sub(current_scroll_offset)
            .min(max_start);
        (start, (start + visible_lines).min(total_lines))
    }
}

/// scrollbar 已移除，此函数为 no-op。
pub(super) fn render_scrollbar(
    _area: Rect,
    _buf: &mut ratatui::buffer::Buffer,
    _total_lines: usize,
    _visible_lines: usize,
    _should_auto_scroll: bool,
    _current_scroll_offset: usize,
) {
    // no-op: scrollbar 已移除，保留右侧 2 列呼吸空间。
}

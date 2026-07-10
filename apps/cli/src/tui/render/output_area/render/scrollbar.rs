use ratatui::{
    layout::Rect,
    widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget},
};

/// 输出区内容为滚动条预留的列数：滚动条本身（1 列）+ 与内容之间的间距（2 列）。
pub(crate) const SCROLLBAR_RESERVE_COLS: u16 = 3;

pub(crate) fn content_area_for_scrollbar(area: Rect, needs_scrollbar: bool) -> Rect {
    if !needs_scrollbar || area.width == 0 {
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

pub(super) fn render_scrollbar(
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    total_lines: usize,
    visible_lines: usize,
    should_auto_scroll: bool,
    current_scroll_offset: usize,
) {
    if total_lines <= visible_lines {
        return;
    }
    let scrollbar_area = Rect {
        x: area.right().saturating_sub(1),
        y: area.top(),
        width: 1,
        height: area.height,
    };
    let max_scroll = total_lines.saturating_sub(visible_lines);
    let current_position = if should_auto_scroll {
        max_scroll
    } else {
        max_scroll.saturating_sub(current_scroll_offset)
    };
    let mut scrollbar_state = ScrollbarState::new(max_scroll).position(current_position);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    StatefulWidget::render(scrollbar, scrollbar_area, buf, &mut scrollbar_state);
}

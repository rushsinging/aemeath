use crate::tui::render::theme;
use ratatui::{layout::Rect, style::Style};

/// 清除区域并写入**显式** `bg=BASE`，绝不把 `Color::Reset` 送到终端。
///
/// crossterm 对 Reset bg 处理不正确，ghostty 系终端会保留该格旧背景
/// （退格后蓝色光标格残留、累积）。与 input area 的既有修复
/// （`render/input_area/render.rs` 的 `set_style(bg(BASE))`）同一模式：
/// 清除格必须携带终端底色的显式 RGB 值，diff 才能真正擦掉残留背景。
pub(super) fn clear_area(area: Rect, buf: &mut ratatui::buffer::Buffer) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].reset();
            buf[(x, y)].set_bg(theme::BASE);
        }
    }
}

pub(super) fn paint_line_fill_styles(
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
    fill_styles: &[Option<Style>],
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    for (row, fill_style) in fill_styles.iter().enumerate() {
        if row >= area.height as usize {
            break;
        }
        let Some(style) = fill_style else {
            continue;
        };
        let y = area.y + row as u16;
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.reset();
                cell.set_style(*style);
            }
        }
    }
}

pub(super) fn trim_line_fill_styles(
    styles: Vec<Option<Style>>,
    height: usize,
) -> Vec<Option<Style>> {
    let len = styles.len();
    if len > height {
        styles.into_iter().skip(len - height).collect()
    } else {
        styles
    }
}

use ratatui::{layout::Rect, style::Style};

pub(super) fn clear_area(area: Rect, buf: &mut ratatui::buffer::Buffer) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].reset();
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

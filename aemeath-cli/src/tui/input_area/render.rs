use super::InputArea;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Widget},
};

impl InputArea {
    /// Render the input area
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let block = self.input_block();
        let inner_area = block.inner(area);
        self.content_width = inner_area.width;
        block.render(area, buf);

        self.textarea.set_block(Block::default());
        self.textarea.render(inner_area, buf);
        self.render_selection(inner_area, buf);
    }

    fn input_block(&self) -> Block<'static> {
        let title = if self.pending_images > 0 {
            format!(" Input [{} image(s) pending] ", self.pending_images)
        } else {
            " Input ".to_string()
        };
        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style)
    }

    fn render_selection(&self, inner_area: Rect, buf: &mut Buffer) {
        let Some(((start_row, start_col), (end_row, end_col))) = self.get_normalized_selection()
        else {
            return;
        };

        let lines = self.textarea.lines();
        let selection_style = Style::default().bg(Color::Blue).fg(Color::White);
        for (row, line_text) in lines.iter().enumerate() {
            if row < start_row || row > end_row {
                continue;
            }
            let line_len = line_text.chars().count();
            let col_from = if row == start_row { start_col } else { 0 };
            let col_to = if row == end_row {
                end_col.min(line_len)
            } else {
                line_len
            };
            highlight_selection_row(inner_area, buf, row, col_from, col_to, selection_style);
        }
    }
}

fn highlight_selection_row(
    inner_area: Rect,
    buf: &mut Buffer,
    row: usize,
    col_from: usize,
    col_to: usize,
    selection_style: Style,
) {
    let screen_y = inner_area.y + row as u16;
    if screen_y >= inner_area.bottom() {
        return;
    }

    for c in col_from..col_to {
        let screen_x = inner_area.x + c as u16;
        if screen_x >= inner_area.right() {
            break;
        }
        if let Some(cell) = buf.cell_mut((screen_x, screen_y)) {
            let ch = cell.symbol().to_string();
            cell.set_style(selection_style);
            if !ch.is_empty() {
                cell.set_symbol(&ch);
            }
        }
    }
}

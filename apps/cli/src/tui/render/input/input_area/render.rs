use super::InputArea;
use crate::tui::render::input::input_area::wrap::{
    display_position_for_anchor, wrap_input_lines_for_width, WrappedInputLine,
};
use crate::tui::render::theme;
use crate::tui::view_model::InputAreaViewModel;
use crate::tui::view_state::InputSelectionViewState;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Widget},
};
use ratatui_textarea::TextArea;

impl InputArea {
    /// Render the input area from a ViewModel.
    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        view_model: &InputAreaViewModel,
        selection: &InputSelectionViewState,
    ) {
        let block = input_block(view_model);
        let inner_area = block.inner(area);
        block.render(area, buf);

        // 用明确的 bg 覆盖 inner_area，确保旧 cursor 的 ACCENT bg 被替换。
        // 不能用 Color::Reset（crossterm 不正确处理），用 BASE（终端底色）替代。
        buf.set_style(inner_area, Style::default().bg(theme::BASE));

        let display_lines =
            wrap_input_lines_for_width(view_model.lines(), inner_area.width as usize);
        let mut textarea = configured_textarea(view_model, &display_lines);
        textarea.set_block(Block::default());
        textarea.render(inner_area, buf);
        render_selection(inner_area, buf, &display_lines, selection);
    }
}

fn input_block(view_model: &InputAreaViewModel) -> Block<'static> {
    let border_style = if view_model.focused {
        Style::default().fg(theme::ACCENT)
    } else {
        Style::default().fg(theme::BORDER)
    };
    Block::default()
        .title(" Input ")
        .borders(Borders::ALL)
        .border_style(border_style)
}

fn render_selection(
    inner_area: Rect,
    buf: &mut Buffer,
    display_lines: &[WrappedInputLine],
    selection: &InputSelectionViewState,
) {
    let Some(((start_row, start_col), (end_row, end_col))) = selection.normalized_selection()
    else {
        return;
    };

    let selection_style = Style::default()
        .bg(theme::SELECTION_BG)
        .fg(theme::SELECTION_FG);
    for (display_row, line) in display_lines.iter().enumerate() {
        if line.original_row < start_row || line.original_row > end_row {
            continue;
        }
        let line_len = line.text.chars().count();
        let line_col_start = line.original_col_start;
        let line_col_end = line_col_start + line_len;
        let select_from = if line.original_row == start_row {
            start_col.max(line_col_start)
        } else {
            line_col_start
        };
        let select_to = if line.original_row == end_row {
            end_col.min(line_col_end)
        } else {
            line_col_end
        };
        if select_from >= select_to {
            continue;
        }
        highlight_selection_row(
            inner_area,
            buf,
            display_row,
            &line.text,
            select_from - line_col_start,
            select_to - line_col_start,
            selection_style,
        );
    }
}

fn configured_textarea(
    vm: &InputAreaViewModel,
    display_lines: &[WrappedInputLine],
) -> TextArea<'static> {
    let mut textarea = TextArea::from(
        display_lines
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<_>>(),
    );
    if let Some(placeholder) = &vm.placeholder {
        textarea.set_placeholder_text(placeholder.clone());
    } else {
        textarea.set_placeholder_text("Type a message... (Enter to send, Alt+Enter for new line)");
    }
    textarea.set_cursor_line_style(Style::default());
    textarea.set_cursor_style(Style::default().bg(theme::ACCENT).fg(theme::SURFACE));
    textarea.move_cursor(ratatui_textarea::CursorMove::Top);
    textarea.move_cursor(ratatui_textarea::CursorMove::Head);
    let (cursor_row, cursor_col) =
        display_position_for_anchor(display_lines, vm.cursor_row, vm.cursor_col);
    for _ in 0..cursor_row {
        textarea.move_cursor(ratatui_textarea::CursorMove::Down);
    }
    for _ in 0..cursor_col {
        textarea.move_cursor(ratatui_textarea::CursorMove::Forward);
    }
    textarea
}

fn highlight_selection_row(
    inner_area: Rect,
    buf: &mut Buffer,
    row: usize,
    line_text: &str,
    col_from: usize,
    col_to: usize,
    selection_style: Style,
) {
    let screen_y = inner_area.y + row as u16;
    if screen_y >= inner_area.bottom() {
        return;
    }

    let screen_col_from = char_col_to_screen_col(line_text, col_from);
    let screen_col_to = char_col_to_screen_col(line_text, col_to);
    for c in screen_col_from..screen_col_to {
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

fn char_col_to_screen_col(line_text: &str, char_col: usize) -> usize {
    crate::tui::render::display::safe_text::str_display_width(
        &line_text.chars().take(char_col).collect::<String>(),
    )
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;

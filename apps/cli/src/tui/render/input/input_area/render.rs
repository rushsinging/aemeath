use super::InputArea;
use crate::tui::render::input::input_area::wrap::{
    display_position_for_anchor, wrap_input_lines_for_width, WrappedInputLine,
};
use crate::tui::render::input::input_render_model::InputRenderModel;
use crate::tui::render::theme;
use crate::tui::view_state::InputSelectionViewState;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Widget},
};
use tui_textarea::TextArea;

impl InputArea {
    /// Render the input area from a model-derived projection.
    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        model: &InputRenderModel,
        selection: &InputSelectionViewState,
    ) {
        let block = input_block(model);
        let inner_area = block.inner(area);
        block.render(area, buf);

        let display_lines = wrap_input_lines_for_width(model.lines(), inner_area.width as usize);
        let mut textarea = configured_textarea(model, &display_lines);
        textarea.set_block(Block::default());
        textarea.render(inner_area, buf);
        render_selection(inner_area, buf, &display_lines, selection);
    }
}

fn input_block(model: &InputRenderModel) -> Block<'static> {
    let title = if model.pending_images > 0 {
        format!(" Input [{} image(s) pending] ", model.pending_images)
    } else {
        " Input ".to_string()
    };
    let border_style = if model.focused {
        Style::default().fg(theme::ACCENT)
    } else {
        Style::default().fg(theme::BORDER)
    };
    Block::default()
        .title(title)
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
    model: &InputRenderModel,
    display_lines: &[WrappedInputLine],
) -> TextArea<'static> {
    let mut textarea = TextArea::from(
        display_lines
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<_>>(),
    );
    if let Some(placeholder) = &model.placeholder {
        textarea.set_placeholder_text(placeholder.clone());
    } else {
        textarea.set_placeholder_text("Type a message... (Enter to send, Alt+Enter for new line)");
    }
    textarea.set_cursor_line_style(Style::default());
    textarea.set_cursor_style(Style::default().bg(theme::ACCENT).fg(theme::SURFACE));
    textarea.move_cursor(tui_textarea::CursorMove::Top);
    textarea.move_cursor(tui_textarea::CursorMove::Head);
    let (cursor_row, cursor_col) =
        display_position_for_anchor(display_lines, model.cursor_row, model.cursor_col);
    for _ in 0..cursor_row {
        textarea.move_cursor(tui_textarea::CursorMove::Down);
    }
    for _ in 0..cursor_col {
        textarea.move_cursor(tui_textarea::CursorMove::Forward);
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
mod tests {
    use super::*;
    use crate::tui::model::input::document::InputDocument;
    use crate::tui::render::display::safe_text::col_to_char_idx;
    use crate::tui::render::input::input_area::selection::text_anchor_for_screen_col;
    use ratatui::buffer::Buffer;

    fn render_model_with_state(
        text: &str,
        pending_images: usize,
        focused: bool,
    ) -> InputRenderModel {
        let mut document = InputDocument::default();
        document.insert_text(text);
        InputRenderModel::from_document(&document, None, pending_images, focused)
    }

    #[test]
    fn test_render_selection_highlights_cjk_to_screen_width_end() {
        let mut input = InputArea::new();
        let model = render_model_with_state("@docs/ bug 33，拖动选中后还是没有高亮", 0, true);
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 3,
        };
        let mut buf = Buffer::empty(area);
        input.render(area, &mut buf, &model, &InputSelectionViewState::default());
        let inner = input.get_inner_area(&area);

        let start = text_anchor_for_screen_col(&model.text, 0, 0);
        let end = text_anchor_for_screen_col(&model.text, 0, 36);
        let mut selection = InputSelectionViewState::default();
        selection.begin_selection(start);
        selection.update_selection(end);
        input.render(area, &mut buf, &model, &selection);
        let selected_end = col_to_char_idx(&model.text, 36);
        let screen_col = char_col_to_screen_col(&model.text, selected_end) - 1;

        assert_eq!(
            buf.cell((inner.x + screen_col as u16, inner.y))
                .unwrap()
                .style()
                .bg,
            Some(theme::SELECTION_BG)
        );
    }

    #[test]
    fn test_render_projects_pending_images_and_focus_from_model() {
        let mut input = InputArea::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 3,
        };
        let mut buf = Buffer::empty(area);
        let model = render_model_with_state("hello", 2, false);

        input.render(area, &mut buf, &model, &InputSelectionViewState::default());

        assert_eq!(buf.cell((2, 0)).unwrap().symbol(), "I");
        assert_eq!(buf.cell((8, 0)).unwrap().symbol(), "[");
        assert_eq!(buf.cell((9, 0)).unwrap().symbol(), "2");
        assert_eq!(buf.cell((0, 0)).unwrap().style().fg, Some(theme::BORDER));
    }

    #[test]
    fn test_render_selection_highlights_wrapped_continuation_line() {
        let mut input = InputArea::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 6,
            height: 4,
        };
        let mut buf = Buffer::empty(area);
        let model = render_model_with_state("abcdef", 0, true);
        let mut selection = InputSelectionViewState::default();
        selection.begin_selection((0, 4));
        selection.update_selection((0, 6));

        input.render(area, &mut buf, &model, &selection);
        let inner = input.get_inner_area(&area);

        assert_eq!(buf.cell((inner.x, inner.y + 1)).unwrap().symbol(), "e");
        assert_eq!(
            buf.cell((inner.x, inner.y + 1)).unwrap().style().bg,
            Some(theme::SELECTION_BG)
        );
        assert_eq!(
            buf.cell((inner.x + 1, inner.y + 1)).unwrap().style().bg,
            Some(theme::SELECTION_BG)
        );
    }
}

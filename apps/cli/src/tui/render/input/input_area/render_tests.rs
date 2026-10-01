use super::*;
use crate::tui::model::input::document::InputDocument;
use crate::tui::render::display::safe_text::col_to_char_idx;
use crate::tui::render::input::input_area::selection::text_anchor_for_screen_col;
use crate::tui::view_assembler::input::InputViewAssembler;
use ratatui::buffer::Buffer;

fn render_vm_with_state(text: &str, focused: bool) -> InputAreaViewModel {
    let mut document = InputDocument::default();
    document.insert_text(text);
    InputViewAssembler::from_document(&document, None, focused)
}

#[test]
fn test_render_selection_highlights_cjk_to_screen_width_end() {
    let mut input = InputArea::new();
    let vm = render_vm_with_state("@docs/ bug 33，拖动选中后还是没有高亮", true);
    let area = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 3,
    };
    let mut buf = Buffer::empty(area);
    input.render(area, &mut buf, &vm, &InputSelectionViewState::default());
    let inner = input.get_inner_area(&area);

    let start = text_anchor_for_screen_col(&vm.text, 0, 0);
    let end = text_anchor_for_screen_col(&vm.text, 0, 36);
    let mut selection = InputSelectionViewState::default();
    selection.begin_selection(start);
    selection.update_selection(end);
    input.render(area, &mut buf, &vm, &selection);
    let selected_end = col_to_char_idx(&vm.text, 36);
    let screen_col = char_col_to_screen_col(&vm.text, selected_end) - 1;

    assert_eq!(
        buf.cell((inner.x + screen_col as u16, inner.y))
            .unwrap()
            .style()
            .bg,
        Some(theme::SELECTION_BG)
    );
}

#[test]
fn test_render_projects_focus_from_vm() {
    let mut input = InputArea::new();
    let area = Rect {
        x: 0,
        y: 0,
        width: 40,
        height: 3,
    };
    let mut buf = Buffer::empty(area);
    let vm = render_vm_with_state("hello", false);

    input.render(area, &mut buf, &vm, &InputSelectionViewState::default());

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
    let vm = render_vm_with_state("abcdef", true);
    let mut selection = InputSelectionViewState::default();
    selection.begin_selection((0, 4));
    selection.update_selection((0, 6));

    input.render(area, &mut buf, &vm, &selection);
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

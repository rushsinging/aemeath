use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::output::selection_overlay::SelRange;
use crate::tui::view_state::output::{OutputViewState, SelectionAnchor};

pub(super) fn sel_range_for_line(
    view: &OutputViewState,
    line: &RenderedLine,
    line_idx: usize,
) -> Option<SelRange> {
    let (start, end) = view.selection_range()?;
    sel_range_for_bounds(start, end, line_idx, line.plain.chars().count())
}

pub(crate) fn sel_range_for_bounds(
    start: SelectionAnchor,
    end: SelectionAnchor,
    line_idx: usize,
    plain_len: usize,
) -> Option<SelRange> {
    let (start_line, start_col) = start;
    let (end_line, end_col) = end;
    if line_idx < start_line || line_idx > end_line {
        return None;
    }
    let start = if line_idx == start_line {
        start_col.as_usize().min(plain_len)
    } else {
        0
    };
    let end = if line_idx == end_line {
        end_col.as_usize().min(plain_len)
    } else {
        plain_len
    };
    (start < end).then_some(SelRange { start, end })
}

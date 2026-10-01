use super::*;

#[test]
fn test_begin_selection_sets_collapsed_anchor_row_width_and_selecting() {
    let mut state = StatusSelectionViewState::default();
    // 正常路径：start==end 落在 char_idx，记录 row/width，置 is_selecting。
    state.begin_selection(StatusBarRow::Context, 4, 120);
    assert_eq!(state.selection_start, Some(4));
    assert_eq!(state.selection_end, Some(4));
    assert_eq!(state.selection_row, StatusBarRow::Context);
    assert_eq!(state.selection_width, 120);
    assert!(state.is_selecting());
    // 边界：行首 char_idx 0 的空选区，Runtime 行 width 0。
    state.begin_selection(StatusBarRow::Runtime, 0, 0);
    assert_eq!(state.selection_start, Some(0));
    assert_eq!(state.selection_end, Some(0));
    assert_eq!(state.selection_row, StatusBarRow::Runtime);
    assert_eq!(state.selection_width, 0);
}

#[test]
fn test_update_selection_moves_end_only_when_selecting() {
    let mut state = StatusSelectionViewState::default();
    // 错误路径：未在选区中时 update 不应改动锚点。
    state.update_selection(5);
    assert_eq!(state.selection_end, None);
    // 正常路径：选区中拖拽更新 end，start/row/width 不变。
    state.begin_selection(StatusBarRow::Runtime, 2, 80);
    state.update_selection(7);
    assert_eq!(state.selection_start, Some(2));
    assert_eq!(state.selection_end, Some(7));
    assert_eq!(state.selection_row, StatusBarRow::Runtime);
    assert_eq!(state.selection_width, 80);
}

#[test]
fn test_selection_range_normalizes_reversed_and_rejects_empty() {
    let mut state = StatusSelectionViewState::default();
    // 错误路径：无锚点返回 None。
    assert_eq!(state.selection_range(), None);
    // 正常路径：start<end 原样返回。
    state.begin_selection(StatusBarRow::Runtime, 2, 0);
    state.update_selection(6);
    assert_eq!(state.selection_range(), Some((2, 6)));
    // 反向：向左拖拽归一化为 start<=end。
    state.begin_selection(StatusBarRow::Runtime, 9, 0);
    state.update_selection(3);
    assert_eq!(state.selection_range(), Some((3, 9)));
    // 边界：空选区（start==end）返回 None（照搬 widget ordered_range）。
    state.begin_selection(StatusBarRow::Runtime, 5, 0);
    assert_eq!(state.selection_range(), None);
}

#[test]
fn test_end_selection_clears_flag_and_returns_range() {
    let mut state = StatusSelectionViewState::default();
    // 错误路径：未选区时 end 返回 None 且标志保持关闭。
    assert_eq!(state.end_selection(), None);
    assert!(!state.is_selecting());
    // 正常路径：结束后清 is_selecting，保留锚点并返回归一化区间。
    state.begin_selection(StatusBarRow::Context, 4, 100);
    state.update_selection(1);
    let range = state.end_selection();
    assert_eq!(range, Some((1, 4)));
    assert!(!state.is_selecting());
    assert!(state.selection_start.is_some());
    assert!(state.selection_end.is_some());
}

#[test]
fn test_clear_selection_resets_all() {
    let mut state = StatusSelectionViewState::default();
    state.begin_selection(StatusBarRow::Context, 2, 120);
    state.update_selection(4);
    state.clear_selection();
    assert_eq!(state.selection_start, None);
    assert_eq!(state.selection_end, None);
    assert_eq!(state.selection_row, StatusBarRow::Runtime);
    assert_eq!(state.selection_width, 0);
    assert!(!state.is_selecting());
}

#[test]
fn test_selection_range_cjk_char_idx_uses_char_units() {
    let mut state = StatusSelectionViewState::default();
    // CJK：char_idx 以字符计数（与 widget col_to_char_idx 折算后一致），
    // "你好世界" 第 1 到第 3 字符。
    state.begin_selection(StatusBarRow::Runtime, 1, 0);
    state.update_selection(3);
    assert_eq!(state.selection_range(), Some((1, 3)));
    // 反向 CJK 锚点归一化。
    state.begin_selection(StatusBarRow::Runtime, 4, 0);
    state.update_selection(2);
    assert_eq!(state.selection_range(), Some((2, 4)));
}

use super::*;

#[test]
fn test_begin_selection_sets_collapsed_anchor_and_selecting() {
    let mut state = InputSelectionViewState::default();
    // 正常路径：start==end 落在 anchor，置 is_selecting。
    state.begin_selection((1, 4));
    assert_eq!(state.selection_start, Some((1, 4)));
    assert_eq!(state.selection_end, Some((1, 4)));
    assert!(state.is_selecting());
    // 边界：行首锚点 (0, 0) 的空选区。
    state.begin_selection((0, 0));
    assert_eq!(state.selection_start, Some((0, 0)));
    assert_eq!(state.selection_end, Some((0, 0)));
}

#[test]
fn test_update_selection_moves_end_only_when_selecting() {
    let mut state = InputSelectionViewState::default();
    // 错误路径：未在选区中时 update 不应改动锚点。
    state.update_selection((0, 5));
    assert_eq!(state.selection_end, None);
    // 正常路径：选区中拖拽更新 end，start 不变。
    state.begin_selection((0, 2));
    state.update_selection((0, 7));
    assert_eq!(state.selection_start, Some((0, 2)));
    assert_eq!(state.selection_end, Some((0, 7)));
}

#[test]
fn test_normalized_selection_handles_reversed_and_rejects_empty() {
    let mut state = InputSelectionViewState::default();
    // 错误路径：无锚点返回 None。
    assert_eq!(state.normalized_selection(), None);
    // 正常路径：同行 start<end 原样返回。
    state.begin_selection((0, 2));
    state.update_selection((0, 6));
    assert_eq!(state.normalized_selection(), Some(((0, 2), (0, 6))));
    // 反向（同行）：向左拖拽归一化为 start<=end。
    state.begin_selection((0, 9));
    state.update_selection((0, 3));
    assert_eq!(state.normalized_selection(), Some(((0, 3), (0, 9))));
    // 反向（跨行）：起点在更大行号 → 归一化交换。
    state.begin_selection((2, 1));
    state.update_selection((1, 5));
    assert_eq!(state.normalized_selection(), Some(((1, 5), (2, 1))));
    // 边界：空选区（start==end）返回 None。
    state.begin_selection((1, 5));
    assert_eq!(state.normalized_selection(), None);
}

#[test]
fn test_end_selection_clears_flag_and_returns_range() {
    let mut state = InputSelectionViewState::default();
    // 错误路径：未选区时 end 返回 None 且标志保持关闭。
    assert_eq!(state.end_selection(), None);
    assert!(!state.is_selecting());
    // 正常路径：结束后清 is_selecting，保留锚点并返回归一化区间。
    state.begin_selection((0, 4));
    state.update_selection((0, 1));
    let range = state.end_selection();
    assert_eq!(range, Some(((0, 1), (0, 4))));
    assert!(!state.is_selecting());
    assert!(state.selection_start.is_some());
    assert!(state.selection_end.is_some());
}

#[test]
fn test_clear_selection_resets_all() {
    let mut state = InputSelectionViewState::default();
    state.begin_selection((1, 2));
    state.update_selection((1, 4));
    state.clear_selection();
    assert_eq!(state.selection_start, None);
    assert_eq!(state.selection_end, None);
    assert!(!state.is_selecting());
}

#[test]
fn test_normalized_selection_cjk_uses_char_units() {
    let mut state = InputSelectionViewState::default();
    // CJK：col 以字符计数（与 widget col_to_char_idx 折算后一致），
    // "你好世界" 第 1 到第 3 字符。
    state.begin_selection((0, 1));
    state.update_selection((0, 3));
    assert_eq!(state.normalized_selection(), Some(((0, 1), (0, 3))));
    // 反向 CJK 锚点归一化。
    state.begin_selection((0, 4));
    state.update_selection((0, 2));
    assert_eq!(state.normalized_selection(), Some(((0, 2), (0, 4))));
}

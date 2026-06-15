use super::*;
use sdk::CharIdx;

fn anchor(line: usize, col: usize) -> SelectionAnchor {
    (line, CharIdx::new(col))
}

#[test]
fn test_default_enables_auto_scroll_for_follow_tail() {
    let state = OutputViewState::default();
    // 默认贴尾：对齐 widget OutputArea::new() 启动 follow-tail 语义。
    assert!(state.auto_scroll);
    // 其余字段保持类型默认值。
    assert_eq!(state.scroll_offset, 0);
    assert!(!state.is_selecting);
    assert_eq!(state.selection_start, None);
    assert_eq!(state.selection_end, None);
    assert_eq!(state.last_visible_height, 0);
    assert_eq!(state.version, 0);
    assert_eq!(state.last_document_total_lines, 0);
}

#[test]
fn test_scroll_up_clamps_and_disables_auto_scroll() {
    let mut state = OutputViewState {
        last_visible_height: 10,
        auto_scroll: true,
        ..Default::default()
    };
    // total=30, max_offset=20，正常路径：偏移累加且关闭 auto_scroll。
    state.scroll_up(5, 30);
    assert_eq!(state.scroll_offset, 5);
    assert!(!state.auto_scroll);
    // 边界：amount 超过 max_offset 时钳制到 max_offset。
    state.scroll_up(100, 30);
    assert_eq!(state.scroll_offset, 20);
    assert!(!state.auto_scroll);
}

#[test]
fn test_scroll_up_resets_when_content_fits() {
    let mut state = OutputViewState {
        last_visible_height: 10,
        scroll_offset: 7,
        auto_scroll: false,
        ..Default::default()
    };
    // max_offset==0（total<=visible）→ 复位并恢复 auto_scroll。
    state.scroll_up(3, 8);
    assert_eq!(state.scroll_offset, 0);
    assert!(state.auto_scroll);
}

#[test]
fn test_scroll_down_decrements_and_reenables_auto_scroll_at_zero() {
    let mut state = OutputViewState {
        scroll_offset: 5,
        auto_scroll: false,
        ..Default::default()
    };
    // 正常路径：递减但未归零，auto_scroll 保持关闭。
    state.scroll_down(2);
    assert_eq!(state.scroll_offset, 3);
    assert!(!state.auto_scroll);
    // 边界：amount 超过当前 offset 时饱和归零并恢复 auto_scroll。
    state.scroll_down(100);
    assert_eq!(state.scroll_offset, 0);
    assert!(state.auto_scroll);
}

#[test]
fn test_scroll_to_bottom_resets_offset_and_auto_scroll() {
    let mut state = OutputViewState {
        scroll_offset: 12,
        auto_scroll: false,
        ..Default::default()
    };
    state.scroll_to_bottom();
    assert_eq!(state.scroll_offset, 0);
    assert!(state.auto_scroll);
}

#[test]
fn test_scroll_to_top_jumps_to_max_offset() {
    let mut state = OutputViewState {
        last_visible_height: 10,
        auto_scroll: true,
        ..Default::default()
    };
    // total=30, max_offset=20：滚到顶后停在 max_offset 且 auto_scroll 关闭。
    state.scroll_to_top(30);
    assert_eq!(state.scroll_offset, 20);
    assert!(!state.auto_scroll);
    // 边界：内容不足一屏时滚到顶等价复位。
    let mut fits = OutputViewState {
        last_visible_height: 10,
        scroll_offset: 4,
        auto_scroll: false,
        ..Default::default()
    };
    fits.scroll_to_top(5);
    assert_eq!(fits.scroll_offset, 0);
    assert!(fits.auto_scroll);
}

#[test]
fn test_sync_document_metrics_keeps_valid_view_scroll() {
    let mut state = OutputViewState {
        scroll_offset: 5,
        auto_scroll: false,
        last_document_total_lines: 100,
        ..Default::default()
    };

    state.sync_document_metrics(100, 20);

    assert_eq!(state.last_visible_height, 20);
    assert_eq!(state.scroll_offset, 5);
    assert!(!state.auto_scroll);
    assert_eq!(state.last_document_total_lines, 100);
}

#[test]
fn test_sync_document_metrics_clamps_stale_offset_and_reenables_auto_scroll() {
    let mut state = OutputViewState {
        scroll_offset: 100,
        auto_scroll: false,
        ..Default::default()
    };

    state.sync_document_metrics(1, 2);

    assert_eq!(state.last_visible_height, 2);
    assert_eq!(state.scroll_offset, 0);
    assert!(state.auto_scroll);
    assert_eq!(state.last_document_total_lines, 1);
}

#[test]
fn test_sync_document_metrics_compensates_growth_when_not_auto_scroll() {
    let mut state = OutputViewState {
        scroll_offset: 5,
        auto_scroll: false,
        last_document_total_lines: 50,
        ..Default::default()
    };

    state.sync_document_metrics(60, 20);

    assert_eq!(state.scroll_offset, 15);
    assert!(!state.auto_scroll);
    assert_eq!(state.last_document_total_lines, 60);
}

#[test]
fn test_sync_document_metrics_clamps_to_max_offset_when_offset_exceeds() {
    let mut state = OutputViewState {
        scroll_offset: 50,
        auto_scroll: false,
        ..Default::default()
    };

    state.sync_document_metrics(30, 10);

    assert_eq!(state.last_visible_height, 10);
    assert_eq!(state.scroll_offset, 20);
    assert!(!state.auto_scroll);
    assert_eq!(state.last_document_total_lines, 30);
}

#[test]
fn test_sync_document_metrics_no_compensation_when_auto_scroll() {
    let mut state = OutputViewState {
        scroll_offset: 0,
        auto_scroll: true,
        last_document_total_lines: 50,
        ..Default::default()
    };

    state.sync_document_metrics(70, 20);

    assert_eq!(state.scroll_offset, 0);
    assert!(state.auto_scroll);
    assert_eq!(state.last_document_total_lines, 70);
}

#[test]
fn test_sync_document_metrics_clamps_small_offset_to_zero_when_content_shrinks() {
    let mut state = OutputViewState {
        scroll_offset: 3,
        auto_scroll: false,
        last_document_total_lines: 50,
        ..Default::default()
    };

    state.sync_document_metrics(5, 10);

    assert_eq!(state.scroll_offset, 0);
    assert!(state.auto_scroll);
    assert_eq!(state.last_document_total_lines, 5);
}

#[test]
fn test_begin_selection_sets_collapsed_anchor_and_selecting() {
    let mut state = OutputViewState::default();
    // 正常路径：start==end 落在同一锚点，is_selecting 置位。
    state.begin_selection(2, CharIdx::new(3));
    assert_eq!(state.selection_start, Some(anchor(2, 3)));
    assert_eq!(state.selection_end, Some(anchor(2, 3)));
    assert!(state.is_selecting());
    // 边界：行首列 0 的空选区。
    state.begin_selection(0, CharIdx::new(0));
    assert_eq!(state.selection_start, Some(anchor(0, 0)));
    assert_eq!(state.selection_end, Some(anchor(0, 0)));
}

#[test]
fn test_update_selection_moves_end_only_when_selecting() {
    let mut state = OutputViewState::default();
    // 错误路径：未在选区中时 update 不应改动锚点。
    state.update_selection(1, CharIdx::new(5));
    assert_eq!(state.selection_end, None);
    // 正常路径：选区中拖拽更新 end，start 不变。
    state.begin_selection(1, CharIdx::new(2));
    state.update_selection(3, CharIdx::new(7));
    assert_eq!(state.selection_start, Some(anchor(1, 2)));
    assert_eq!(state.selection_end, Some(anchor(3, 7)));
}

#[test]
fn test_selection_range_normalizes_reversed_anchors() {
    let mut state = OutputViewState::default();
    // 正常路径：start<end 时原样返回。
    state.begin_selection(1, CharIdx::new(2));
    state.update_selection(4, CharIdx::new(0));
    assert_eq!(state.selection_range(), Some((anchor(1, 2), anchor(4, 0))));
    // 反向：向上/向左拖拽时归一化为 start<=end。
    state.begin_selection(4, CharIdx::new(6));
    state.update_selection(1, CharIdx::new(1));
    assert_eq!(state.selection_range(), Some((anchor(1, 1), anchor(4, 6))));
    // 同行反向列。
    state.begin_selection(2, CharIdx::new(8));
    state.update_selection(2, CharIdx::new(3));
    assert_eq!(state.selection_range(), Some((anchor(2, 3), anchor(2, 8))));
}

#[test]
fn test_selection_range_empty_and_missing() {
    let mut state = OutputViewState::default();
    // 错误路径：无锚点返回 None。
    assert_eq!(state.selection_range(), None);
    // 边界：空选区（start==end）仍返回该对。
    state.begin_selection(2, CharIdx::new(5));
    assert_eq!(state.selection_range(), Some((anchor(2, 5), anchor(2, 5))));
}

#[test]
fn test_end_selection_clears_flag_and_returns_range() {
    let mut state = OutputViewState::default();
    // 错误路径：未选区时 end 返回 None 且标志保持关闭。
    assert_eq!(state.end_selection(), None);
    assert!(!state.is_selecting());
    // 正常路径：结束后清 is_selecting，保留锚点并返回归一化区间。
    state.begin_selection(0, CharIdx::new(4));
    state.update_selection(0, CharIdx::new(1));
    let range = state.end_selection();
    assert_eq!(range, Some((anchor(0, 1), anchor(0, 4))));
    assert!(!state.is_selecting());
    assert!(state.selection_start.is_some());
    assert!(state.selection_end.is_some());
}

#[test]
fn test_clear_selection_resets_all() {
    let mut state = OutputViewState::default();
    state.begin_selection(1, CharIdx::new(2));
    state.update_selection(3, CharIdx::new(4));
    state.clear_selection();
    assert_eq!(state.selection_start, None);
    assert_eq!(state.selection_end, None);
    assert!(!state.is_selecting());
}

#[test]
fn test_select_word_sets_word_bounds_and_selecting() {
    let mut state = OutputViewState::default();
    // 正常路径：start/end 落在同一逻辑行的词边界，置 is_selecting。
    state.select_word(2, CharIdx::new(3), CharIdx::new(7));
    assert_eq!(state.selection_start, Some(anchor(2, 3)));
    assert_eq!(state.selection_end, Some(anchor(2, 7)));
    assert!(state.is_selecting());
    // 边界：单字符词（start+1==end）。
    state.select_word(0, CharIdx::new(0), CharIdx::new(1));
    assert_eq!(state.selection_range(), Some((anchor(0, 0), anchor(0, 1))));
}

#[test]
fn test_selection_range_cjk_char_idx_uses_char_units() {
    let mut state = OutputViewState::default();
    // CJK：CharIdx 以字符计数，"你好世界" 第 1 到第 3 字符。
    state.begin_selection(0, CharIdx::new(1));
    state.update_selection(0, CharIdx::new(3));
    assert_eq!(state.selection_range(), Some((anchor(0, 1), anchor(0, 3))));
    // 反向 CJK 锚点归一化。
    state.begin_selection(0, CharIdx::new(4));
    state.update_selection(0, CharIdx::new(2));
    assert_eq!(state.selection_range(), Some((anchor(0, 2), anchor(0, 4))));
}

#[test]
fn test_last_document_total_lines_default_zero() {
    let state = OutputViewState::default();
    assert_eq!(state.last_document_total_lines, 0);
}

#[test]
fn test_scroll_pin_growth_compensates_offset() {
    let state = OutputViewState {
        last_visible_height: 10,
        scroll_offset: 5,
        auto_scroll: false,
        last_document_total_lines: 30,
        ..Default::default()
    };
    // 内容从 30 行增长到 40 行（Δ=10），scroll_offset 应增加 10。
    // 但 max_offset = 40 - 10 = 30，5+10=15 < 30，不触发钳制。
    let growth = 40usize.saturating_sub(state.last_document_total_lines);
    assert!(!state.auto_scroll);
    assert_eq!(growth, 10);
    let expected = state.scroll_offset.saturating_add(growth);
    assert_eq!(expected, 15);
}

#[test]
fn test_scroll_pin_shrink_no_compensation() {
    let state = OutputViewState {
        last_visible_height: 10,
        scroll_offset: 12,
        auto_scroll: false,
        last_document_total_lines: 30,
        ..Default::default()
    };
    // 内容从 30 行收缩到 20 行，growth=0（saturating_sub），不应补偿。
    let new_total = 20usize;
    let growth = new_total.saturating_sub(state.last_document_total_lines);
    assert_eq!(growth, 0);
    // offset(12) 超出 max_offset(20-10=10)，钳制后应为 10。
    let max_offset = new_total.saturating_sub(state.last_visible_height);
    assert_eq!(max_offset, 10);
    let clamped = state.scroll_offset.min(max_offset);
    assert_eq!(clamped, 10);
}

#[test]
fn test_scroll_pin_auto_scroll_true_skips_compensation() {
    let state = OutputViewState {
        last_visible_height: 10,
        scroll_offset: 0,
        auto_scroll: true,
        last_document_total_lines: 30,
        ..Default::default()
    };
    // auto_scroll=true 时不触发补偿。
    assert!(state.auto_scroll);
    let growth = 40usize.saturating_sub(state.last_document_total_lines);
    assert_eq!(growth, 10);
    let compensated = if !state.auto_scroll {
        state.scroll_offset.saturating_add(growth)
    } else {
        state.scroll_offset
    };
    assert_eq!(compensated, 0);
}

use std::collections::HashSet;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OutputViewState {
    pub scroll_offset: usize,
    pub follow_tail: bool,
    pub auto_scroll: bool,
    pub is_selecting: bool,
    pub selection_start: Option<SelectedTextRange>,
    pub selection_end: Option<SelectedTextRange>,
    pub selected_text_range: Option<SelectedTextRange>,
    pub screen_line_map: Vec<ScreenLineMapEntry>,
    pub last_visible_height: usize,
    pub render_revision: u64,
    pub collapsed_blocks: HashSet<String>,
    pub version: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedTextRange {
    pub start_block_key: String,
    pub start_offset: usize,
    pub end_block_key: String,
    pub end_offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScreenLineMapEntry {
    pub block_key: String,
    pub line_index: usize,
}

impl OutputViewState {
    /// 向上滚动指定行数。
    ///
    /// 逻辑与 `render::output_area::scroll::OutputArea::scroll_up` 等价：
    /// view_state 不持有 document，故总行数由调用方传入。
    /// - `max_offset = total_lines - last_visible_height`（饱和减）；
    /// - `max_offset == 0`（内容不超过可见高度）时复位 offset=0 并恢复 auto_scroll；
    /// - 否则关闭 auto_scroll，并将 offset 钳制到 `max_offset`。
    pub fn scroll_up(&mut self, amount: usize, total_lines: usize) {
        self.auto_scroll = false;
        let max_offset = total_lines.saturating_sub(self.last_visible_height);
        self.scroll_offset = self.scroll_offset.saturating_add(amount).min(max_offset);
        if max_offset == 0 {
            self.scroll_offset = 0;
            self.auto_scroll = true;
        }
    }

    /// 向下滚动指定行数。offset 归零时恢复 auto_scroll。
    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        if self.scroll_offset == 0 {
            self.auto_scroll = true;
        }
    }

    /// 滚动到底部：offset 归零并恢复 auto_scroll。
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    /// 滚动到顶部：等价于向上滚动 `total_lines` 行（钳制后落在 max_offset）。
    pub fn scroll_to_top(&mut self, total_lines: usize) {
        self.scroll_up(total_lines, total_lines);
    }
}

#[cfg(test)]
mod tests {
    use super::OutputViewState;

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
}

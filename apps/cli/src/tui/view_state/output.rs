use sdk::CharIdx;

/// 选区锚点：`(逻辑行, plain CharIdx)`（#63 坐标系）。
///
/// 与 widget `render::output_area::OutputArea.selection_start/end` 同型，
/// 屏幕坐标 → 锚点的折算（gutter_cols 补偿 + plain 列换算）保留在 widget
/// （依赖 render 期的 screen_line_map/document），view_state 只持纯锚点状态。
pub type SelectionAnchor = (usize, CharIdx);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputViewState {
    pub scroll_offset: usize,
    pub auto_scroll: bool,
    pub is_selecting: bool,
    pub selection_start: Option<SelectionAnchor>,
    pub selection_end: Option<SelectionAnchor>,
    pub last_visible_height: usize,
    pub last_document_total_lines: usize,
    pub version: u64,
}

impl Default for OutputViewState {
    /// `auto_scroll` 默认 `true`，对齐 widget `OutputArea::new()` 的启动贴尾语义
    /// （view_state 现为滚动真相，S2 Task 3）：避免启动内容超过可见高度时
    /// 首帧出现非贴尾闪烁。其余字段保持类型默认值。
    fn default() -> Self {
        Self {
            scroll_offset: 0,
            auto_scroll: true,
            is_selecting: false,
            selection_start: None,
            selection_end: None,
            last_visible_height: 0,
            last_document_total_lines: 0,
            version: 0,
        }
    }
}

impl OutputViewState {
    /// 向上滚动指定行数。
    ///
    /// view_state 是滚动真相；不持有 document，故总行数由调用方传入。
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

    /// 开始选区。锚点 `(line, col)` 由调用方据 render 期的 screen_line_map
    /// 折算屏幕坐标（含 gutter_cols 补偿）后传入。
    ///
    /// 等价于 widget `start_selection` 的状态更新部分：
    /// 置 `is_selecting=true`，start/end 同时落在锚点（空选区）。
    pub fn begin_selection(&mut self, line: usize, col: CharIdx) {
        self.selection_start = Some((line, col));
        self.selection_end = Some((line, col));
        self.is_selecting = true;
    }

    /// 拖拽更新选区终点。仅在 `is_selecting` 时生效（与 widget `update_selection` 等价）。
    /// 锚点 `(line, col)` 由调用方折算后传入。
    pub fn update_selection(&mut self, line: usize, col: CharIdx) {
        if !self.is_selecting {
            return;
        }
        self.selection_end = Some((line, col));
    }

    /// 结束选区拖拽：清 `is_selecting` 标志并返回归一化后的锚点对（供调用方取文本）。
    ///
    /// 与 widget `end_selection` 的差异：widget 取 plain 文本（依赖 render 期 document）
    /// 并随后清空 start/end；本方法只管状态机，保留锚点供调用方借 widget 取文本，
    /// 取完文本后由调用方调 `clear_selection` 清空。
    pub fn end_selection(&mut self) -> Option<(SelectionAnchor, SelectionAnchor)> {
        self.is_selecting = false;
        self.selection_range()
    }

    /// 清空选区：start/end 置空且 `is_selecting=false`（与 widget `clear_selection` 等价）。
    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.is_selecting = false;
    }

    /// 整词选区。词边界 `[word_start, word_end)` 由调用方据行内容计算后传入
    /// （边界扫描依赖 render 期行文本，留在 widget `select_word`）。
    /// 与 widget 一致：置 `is_selecting=true` 且 start/end 落在同一逻辑行的词边界。
    pub fn select_word(&mut self, line: usize, word_start: CharIdx, word_end: CharIdx) {
        self.selection_start = Some((line, word_start));
        self.selection_end = Some((line, word_end));
        self.is_selecting = true;
    }

    /// 是否正在拖拽选区。
    pub fn is_selecting(&self) -> bool {
        self.is_selecting
    }

    /// 归一化后的选区锚点对 `(start, end)`，保证 `start <= end`（逻辑行优先、同行比 CharIdx）。
    ///
    /// 空选区（start==end）仍返回该对；调用方据需自行判定是否为空。
    /// 与 widget `get_selected_text` 的归一化分支等价。
    pub fn selection_range(&self) -> Option<(SelectionAnchor, SelectionAnchor)> {
        let start = self.selection_start?;
        let end = self.selection_end?;
        let (start_line, start_col) = start;
        let (end_line, end_col) = end;
        if start_line < end_line || (start_line == end_line && start_col <= end_col) {
            Some((start, end))
        } else {
            Some((end, start))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{OutputViewState, SelectionAnchor};
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
}

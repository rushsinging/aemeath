//! Status 选区 view_state（#59 S4 / #70 phase 2）。
//!
//! 对齐 S2 `output.rs` 范式：选区真相收敛到 view_state 锚点状态机，
//! `render/status/bar.rs` 渲染时直接消费该投影，widget 不再保存第二份 status selection
//! mirror。
//!
//! 坐标模型照搬现 `display/status_bar_selection.rs`，无行为漂移：
//! - `row`：`StatusBarRow`（Runtime | Context），标识选区所在状态栏逻辑行；
//! - `char_idx`：plain 文本字符索引（非屏幕列、非字节）。屏幕列 → char_idx 的折算
//!   （`screen_col_to_char_idx` 依赖 render 期 `build_full_text`/`context_row_text`）
//!   保留在 widget 只读借用，view_state 只持已折算的 char_idx 锚点（对齐 output
//!   的 `screen_to_anchor` 留 widget）；
//! - `width`：折算 Context 行文本所需的渲染宽度（render 期布局数据，作为 view_state
//!   元数据供后续 render/copy 使用）。

use crate::tui::render::status::StatusBarRow;

/// Status 选区视图状态：锚点状态机，对齐 widget status 选区坐标模型。
///
/// `selection_start`/`selection_end` 为 plain 文本 char_idx；`row`/`width` 为
/// 折算上下文元数据（由调用方据 render 期布局折算后传入）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusSelectionViewState {
    pub is_selecting: bool,
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
    pub selection_row: StatusBarRow,
    pub selection_width: u16,
}

impl Default for StatusSelectionViewState {
    /// 对齐 widget `StatusBar::new()`：未选区，row 缺省 `Runtime`，width 0。
    fn default() -> Self {
        Self {
            is_selecting: false,
            selection_start: None,
            selection_end: None,
            selection_row: StatusBarRow::Runtime,
            selection_width: 0,
        }
    }
}

impl StatusSelectionViewState {
    /// 开始选区。`char_idx`/`row`/`width` 由调用方据 render 期布局折算屏幕坐标
    /// （`screen_col_to_char_idx`）后传入。
    ///
    /// 等价于 widget `start_selection_at` 的状态更新部分：记录 row/width，
    /// start/end 同时落在 char_idx（空选区），置 `is_selecting=true`。
    pub fn begin_selection(&mut self, row: StatusBarRow, char_idx: usize, width: u16) {
        self.selection_row = row;
        self.selection_width = width;
        self.selection_start = Some(char_idx);
        self.selection_end = Some(char_idx);
        self.is_selecting = true;
    }

    /// 拖拽更新选区终点。仅在 `is_selecting` 时生效（与 widget `update_selection_at` 等价）。
    /// `char_idx` 由调用方据已记录的 `selection_row`/`width` 折算后传入；row 不变。
    pub fn update_selection(&mut self, char_idx: usize) {
        if !self.is_selecting {
            return;
        }
        self.selection_end = Some(char_idx);
    }

    /// 结束选区拖拽：清 `is_selecting` 标志并返回归一化后的 char_idx 区间（供调用方取文本）。
    ///
    /// 与 widget `end_selection` 的差异：widget 取 plain 文本（依赖 render 期 `line_text`）
    /// 并随后清空 start/end/width；本方法只管状态机，保留锚点供调用方借 widget 取文本，
    /// 取完文本后由调用方调 `clear_selection` 清空（对齐 output `end_selection`）。
    pub fn end_selection(&mut self) -> Option<(usize, usize)> {
        self.is_selecting = false;
        self.selection_range()
    }

    /// 清空选区：start/end 置空、row 复位 `Runtime`、width 归零、`is_selecting=false`
    /// （与 widget `clear_selection` 等价）。
    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.selection_row = StatusBarRow::Runtime;
        self.selection_width = 0;
        self.is_selecting = false;
    }

    /// 是否正在拖拽选区。
    pub fn is_selecting(&self) -> bool {
        self.is_selecting
    }

    /// 归一化后的选区 char_idx 区间 `(start, end)`，保证 `start <= end`。
    ///
    /// 与 widget `get_selected_text` 的 `ordered_range` 归一化分支等价；
    /// 但**空选区（start==end）返回 `None`**（照搬 widget `ordered_range` 语义：
    /// 折叠选区无文本可取）。
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let start = self.selection_start?;
        let end = self.selection_end?;
        let (start, end) = if start < end {
            (start, end)
        } else {
            (end, start)
        };
        if start == end {
            None
        } else {
            Some((start, end))
        }
    }
}

#[cfg(test)]
mod tests {
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
}

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
#[path = "status_tests.rs"]
mod tests;

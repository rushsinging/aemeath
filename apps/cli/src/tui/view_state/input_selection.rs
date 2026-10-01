//! Input 选区 view_state（#59 S4；#70 phase 2 去除 InputArea selection mirror）。
//!
//! 对齐 S2 `output.rs` / T1 `status.rs` 范式：选区真相收敛到 view_state 锚点
//! 状态机。`InputArea` 不再保存 input selection mirror；render 直接消费本 view_state。
//!
//! 坐标模型照搬现 `render/input/input_area/selection.rs`，无行为漂移：
//! - 锚点为 textarea `(row, col)`（usize, usize）：`row` 为 textarea 行号，`col`
//!   为该行 plain 文本字符索引（非屏幕列、非字节）。屏幕坐标 → `(row, col)` 的折算
//!   （`textarea_pos`：减 inner_area 偏移 + `col_to_char_idx`）依赖 render 期
//!   input text 投影，保留在 `InputArea::screen_to_input_anchor` 只读折算函数中。

/// 选区锚点：textarea `(row, col)`。`row` 为 textarea 行号，`col` 为该行 plain
/// 文本字符索引。`InputArea` render 直接消费该锚点。
pub type InputAnchor = (usize, usize);

/// Input 选区视图状态：锚点状态机，对齐 widget input 选区坐标模型。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputSelectionViewState {
    pub is_selecting: bool,
    pub selection_start: Option<InputAnchor>,
    pub selection_end: Option<InputAnchor>,
}

impl InputSelectionViewState {
    /// 开始选区。`anchor` 由调用方据 render 期 textarea 布局折算屏幕坐标
    /// （widget `textarea_pos`）后传入。
    ///
    /// 等价于 widget `start_selection` 的状态更新部分：start/end 同时落在 anchor
    /// （空选区），置 `is_selecting=true`。
    pub fn begin_selection(&mut self, anchor: InputAnchor) {
        self.selection_start = Some(anchor);
        self.selection_end = Some(anchor);
        self.is_selecting = true;
    }

    /// 拖拽更新选区终点。仅在 `is_selecting` 时生效（与 widget `update_selection` 等价）。
    /// `anchor` 由调用方据 render 期 textarea 折算后传入。
    pub fn update_selection(&mut self, anchor: InputAnchor) {
        if !self.is_selecting {
            return;
        }
        self.selection_end = Some(anchor);
    }

    /// 结束选区拖拽：清 `is_selecting` 标志并返回归一化后的锚点区间（供调用方取文本）。
    ///
    /// 与 widget `end_selection` 的差异：widget 取 plain 文本（依赖 render 期
    /// `textarea.lines()`）并随后清空 start/end；本方法只管状态机，保留锚点供调用方
    /// 借 widget 取文本，取完后由调用方调 `clear_selection` 清空（对齐 output/status
    /// `end_selection`）。
    pub fn end_selection(&mut self) -> Option<(InputAnchor, InputAnchor)> {
        self.is_selecting = false;
        self.normalized_selection()
    }

    /// 清空选区：start/end 置空、`is_selecting=false`（与 widget `clear_selection` 等价）。
    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.is_selecting = false;
    }

    /// 是否正在拖拽选区。
    pub fn is_selecting(&self) -> bool {
        self.is_selecting
    }

    /// 归一化后的选区区间 `(start, end)`，保证 `start <= end`（按 `(row, col)` 字典序）。
    ///
    /// 与 widget `get_normalized_selection` 等价：空选区（start==end）返回 `None`
    /// （折叠选区无文本可取）。
    pub fn normalized_selection(&self) -> Option<(InputAnchor, InputAnchor)> {
        let start = self.selection_start?;
        let end = self.selection_end?;
        if start == end {
            return None;
        }
        if start.0 < end.0 || (start.0 == end.0 && start.1 < end.1) {
            Some((start, end))
        } else {
            Some((end, start))
        }
    }
}

#[cfg(test)]
#[path = "input_selection_tests.rs"]
mod tests;

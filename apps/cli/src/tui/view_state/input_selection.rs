//! Input 选区 view_state（#59 S4）。
//!
//! 对齐 S2 `output.rs` / T1 `status.rs` 范式：选区真相收敛到 view_state 锚点
//! 状态机，widget `render/input/input_area/` 的 `is_selecting`/`selection_start`/
//! `selection_end` 降为只读镜像，由 `adapter/input_widget.rs::apply_input_selection_to_widget`
//! 单向写回（T4 接线）。
//!
//! 坐标模型照搬现 `render/input/input_area/selection.rs`，无行为漂移：
//! - 锚点为 textarea `(row, col)`（usize, usize）：`row` 为 textarea 行号，`col`
//!   为该行 plain 文本字符索引（非屏幕列、非字节）。屏幕坐标 → `(row, col)` 的折算
//!   （`textarea_pos`：减 inner_area 偏移 + `col_to_char_idx`）依赖 render 期
//!   `textarea.lines()`，保留在 widget 只读借用，view_state 只持已折算的锚点
//!   （对齐 output 的 `screen_to_anchor` / status 的 `screen_col_to_char_idx` 留 widget）。

/// 选区锚点：textarea `(row, col)`。`row` 为 textarea 行号，`col` 为该行 plain
/// 文本字符索引。与 widget `InputArea.selection_start/end` 同型。
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
mod tests {
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
}

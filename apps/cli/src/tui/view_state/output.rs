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
    /// 是否已展开全部消息（懒加载：scroll_to_top 时设为 true，跳过 MAX_RENDER_LINES 裁剪）。
    pub expanded: bool,
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
            expanded: false,
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
    /// 同时展开懒加载（设置 expanded=true），下次 refresh 时跳过 MAX_RENDER_LINES 裁剪。
    pub fn scroll_to_top(&mut self, total_lines: usize) {
        self.expanded = true;
        self.scroll_up(total_lines, total_lines);
    }

    /// 同步 document 指标并维护滚动真相。
    ///
    /// 每帧渲染前由 App 根据 Output document 与 layout/live-status 投影调用：
    /// - `visible_height` 直接来自当前 layout，不经 OutputArea 反喂；
    /// - document 增长且 `auto_scroll=false` 时补偿 offset，保持视窗内容固定；
    /// - offset 钳制到当前最大可滚动范围；offset 归零时恢复贴尾。
    pub fn sync_document_metrics(&mut self, total_lines: usize, visible_height: usize) {
        self.last_visible_height = visible_height;
        if !self.auto_scroll {
            let growth = total_lines.saturating_sub(self.last_document_total_lines);
            self.scroll_offset = self.scroll_offset.saturating_add(growth);
        }
        self.last_document_total_lines = total_lines;

        let max_offset = total_lines.saturating_sub(self.last_visible_height);
        self.scroll_offset = self.scroll_offset.min(max_offset);
        if self.scroll_offset == 0 {
            self.auto_scroll = true;
        }
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
#[path = "output_tests.rs"]
mod tests;

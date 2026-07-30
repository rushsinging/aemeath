use sdk::CharIdx;

const INITIAL_RENDER_LINES: usize = 1_000;
const HISTORY_LOAD_PERCENT: usize = 60;
const MIN_HISTORY_LOAD_BATCH_LINES: usize = 15;
const MAX_RENDER_LINES: usize = 3_000;

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
    /// 当前允许渲染的历史行预算；达到加载阈值后按视口比例增长。
    pub(crate) render_line_limit: usize,
    /// 最近一次完整源文档的行数，用于判断是否还有更早历史并补偿流式增长。
    pub(crate) source_total_lines: usize,
    /// 源文档尚未首次构建时收到的顶部加载请求；观察到完整源后兑现一个批次。
    pub(crate) pending_load_older: bool,
    /// 已向更早历史移动的窗口尾部行数（窗口达到上限后的滑动偏移）。
    pub(crate) history_window_tail_offset: usize,
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
            render_line_limit: INITIAL_RENDER_LINES,
            source_total_lines: 0,
            pending_load_older: false,
            history_window_tail_offset: 0,
        }
    }
}

impl OutputViewState {
    /// 将完整历史底部归一化为唯一状态：贴尾、默认窗口预算、无待处理历史加载。
    fn normalize_latest_bottom(&mut self) {
        self.auto_scroll = true;
        self.render_line_limit = INITIAL_RENDER_LINES;
        self.pending_load_older = false;
    }

    fn history_load_batch_lines(&self) -> usize {
        self.last_visible_height
            .saturating_mul(HISTORY_LOAD_PERCENT)
            .saturating_add(99)
            .checked_div(100)
            .unwrap_or(usize::MAX)
            .max(MIN_HISTORY_LOAD_BATCH_LINES)
    }

    /// 向上滚动指定行数。
    ///
    /// view_state 是滚动真相；不持有 document，故总行数由调用方传入。
    /// - `max_offset = total_lines - last_visible_height`（饱和减）；
    /// - `max_offset == 0`（内容不超过可见高度）时复位 offset=0 并恢复 auto_scroll；
    /// - 否则关闭 auto_scroll，并将 offset 钳制到 `max_offset`。
    pub fn scroll_up(&mut self, amount: usize, total_lines: usize) {
        let before_offset = self.scroll_offset;
        let before_auto_scroll = self.auto_scroll;
        self.auto_scroll = false;
        let max_offset = total_lines.saturating_sub(self.last_visible_height);
        self.scroll_offset = self.scroll_offset.saturating_add(amount).min(max_offset);
        if max_offset == 0 {
            self.scroll_offset = 0;
            self.auto_scroll = true;
        }
        crate::tui::log_debug!(
            "tui.output.scroll_transition action=up amount={} total_lines={} visible_height={} max_offset={} before_offset={} after_offset={} before_auto_scroll={} after_auto_scroll={} render_limit={} source_total_lines={} tail_offset={}",
            amount,
            total_lines,
            self.last_visible_height,
            max_offset,
            before_offset,
            self.scroll_offset,
            before_auto_scroll,
            self.auto_scroll,
            self.render_line_limit,
            self.source_total_lines,
            self.history_window_tail_offset
        );
    }

    /// 向下滚动指定行数。
    ///
    /// 先消费当前历史窗口内的 `scroll_offset`；到达窗口底部后，若窗口仍偏离
    /// 最新历史，则按同一视口比例批量向最新方向滑动，并用新增的窗口尾部行数补偿
    /// `scroll_offset`，使视口连续跨窗。仅在最新窗口底部恢复 `auto_scroll`。
    /// 返回值表示历史窗口是否发生变化，调用方据此重建 document。
    pub fn scroll_down(&mut self, amount: usize) -> bool {
        let before_offset = self.scroll_offset;
        let before_auto_scroll = self.auto_scroll;
        let before_limit = self.render_line_limit;
        let before_tail_offset = self.history_window_tail_offset;
        let mut remaining = amount;
        let within_window = remaining.min(self.scroll_offset);
        self.scroll_offset -= within_window;
        remaining -= within_window;

        let mut window_changed = false;
        while remaining > 0 && self.history_window_tail_offset > 0 {
            let shifted_lines = self
                .history_load_batch_lines()
                .min(self.history_window_tail_offset);
            self.history_window_tail_offset -= shifted_lines;
            self.scroll_offset = shifted_lines;
            window_changed = true;

            let across_window = remaining.min(self.scroll_offset);
            self.scroll_offset -= across_window;
            remaining -= across_window;
        }

        let reached_latest_bottom = self.scroll_offset == 0 && self.history_window_tail_offset == 0;
        if reached_latest_bottom {
            self.normalize_latest_bottom();
        } else {
            self.auto_scroll = false;
        }
        window_changed |= self.render_line_limit != before_limit;
        crate::tui::log_debug!(
            "tui.output.scroll_transition action=down amount={} remaining={} total_lines={} visible_height={} before_offset={} after_offset={} before_auto_scroll={} after_auto_scroll={} before_limit={} after_limit={} source_total_lines={} before_tail_offset={} after_tail_offset={} window_changed={}",
            amount,
            remaining,
            self.last_document_total_lines,
            self.last_visible_height,
            before_offset,
            self.scroll_offset,
            before_auto_scroll,
            self.auto_scroll,
            before_limit,
            self.render_line_limit,
            self.source_total_lines,
            before_tail_offset,
            self.history_window_tail_offset,
            window_changed
        );
        window_changed
    }

    /// 滚动到底部：offset 归零、恢复 auto_scroll，并恢复默认历史窗口预算。
    pub fn scroll_to_bottom(&mut self) {
        let before_offset = self.scroll_offset;
        let before_auto_scroll = self.auto_scroll;
        let before_limit = self.render_line_limit;
        let before_tail_offset = self.history_window_tail_offset;
        self.scroll_offset = 0;
        self.history_window_tail_offset = 0;
        self.normalize_latest_bottom();
        crate::tui::log_debug!(
            "tui.output.scroll_transition action=bottom before_offset={} after_offset=0 before_auto_scroll={} after_auto_scroll=true before_limit={} after_limit={} source_total_lines={} before_tail_offset={} after_tail_offset=0",
            before_offset,
            before_auto_scroll,
            before_limit,
            self.render_line_limit,
            self.source_total_lines,
            before_tail_offset
        );
    }

    /// 滚动到当前已渲染文档顶部，并在仍有更早历史时请求一个批次。
    pub fn scroll_to_top(&mut self, total_lines: usize) -> bool {
        self.scroll_up(total_lines, total_lines);
        self.request_load_older_at_top()
    }

    /// 接近当前已渲染文档顶部时预加载一个更早历史批次。
    ///
    /// 阈值与单批加载量一致，使新增历史在用户到顶前准备好；窗口仍有超过
    /// 一个批次的可滚动内容时不重建 document。
    pub fn try_load_older_near_top(&mut self, total_lines: usize) -> bool {
        let max_offset = total_lines.saturating_sub(self.last_visible_height);
        let remaining_lines_above = max_offset.saturating_sub(self.scroll_offset);
        remaining_lines_above <= self.history_load_batch_lines() && self.request_load_older_at_top()
    }

    pub fn request_load_older_at_top(&mut self) -> bool {
        let before_limit = self.render_line_limit;
        let before_tail_offset = self.history_window_tail_offset;
        if self.source_total_lines == 0 {
            self.pending_load_older = true;
            crate::tui::log_debug!(
                "tui.history.request_older deferred source_total_lines=0 limit={} offset={} tail_offset={} pending_load_older=true",
                self.render_line_limit,
                self.scroll_offset,
                self.history_window_tail_offset
            );
            return false;
        }
        let expanded = if self.render_line_limit < MAX_RENDER_LINES {
            self.load_older_batch()
        } else {
            let max_tail_offset = self
                .source_total_lines
                .saturating_sub(self.render_line_limit);
            self.history_window_tail_offset = self
                .history_window_tail_offset
                .saturating_add(self.history_load_batch_lines())
                .min(max_tail_offset);
            self.history_window_tail_offset > before_tail_offset
        };
        if expanded {
            crate::tui::log_debug!(
                "tui.history.request_older source_total_lines={} expanded=true before_limit={} after_limit={} offset={} before_tail_offset={} after_tail_offset={} pending_load_older={}",
                self.source_total_lines,
                before_limit,
                self.render_line_limit,
                self.scroll_offset,
                before_tail_offset,
                self.history_window_tail_offset,
                self.pending_load_older
            );
        } else {
            crate::tui::log_trace!(
                "tui.history.request_older source_total_lines={} expanded=false limit={} offset={} tail_offset={} pending_load_older={}",
                self.source_total_lines,
                self.render_line_limit,
                self.scroll_offset,
                self.history_window_tail_offset,
                self.pending_load_older
            );
        }
        expanded
    }

    pub fn render_line_limit(&self) -> usize {
        self.render_line_limit
    }

    /// 在裁剪前观察完整源文档。用户已离开底部时，流式新增行同步扩大预算，
    /// 使已渲染窗口只在顶部增加历史批次、在底部增加实时输出，两者都可由 offset 补偿固定视口。
    pub fn observe_source_document(&mut self, source_total_lines: usize) {
        let growth = source_total_lines.saturating_sub(self.source_total_lines);
        if !self.auto_scroll && growth > 0 {
            self.render_line_limit = self
                .render_line_limit
                .saturating_add(growth)
                .min(MAX_RENDER_LINES);
        }
        self.source_total_lines = source_total_lines;
        self.render_line_limit = self
            .render_line_limit
            .min(source_total_lines.max(INITIAL_RENDER_LINES));
        if self.pending_load_older {
            self.pending_load_older = false;
            let expanded = self.load_older_batch();
            crate::tui::log_debug!(
                "tui.history.observe_source fulfilled_pending=true source_total_lines={} growth={} expanded={} limit={}",
                source_total_lines,
                growth,
                expanded,
                self.render_line_limit
            );
        }
    }

    fn load_older_batch(&mut self) -> bool {
        let next_limit = self
            .render_line_limit
            .saturating_add(self.history_load_batch_lines())
            .min(self.source_total_lines)
            .min(MAX_RENDER_LINES);
        if next_limit <= self.render_line_limit {
            return false;
        }
        self.render_line_limit = next_limit;
        true
    }

    /// 在历史窗口 document 重建完成后，立即将原可见首行锚定到相同屏幕位置。
    ///
    /// `anchor_line` 是同一语义行在新 document 中的逻辑行索引。同步更新
    /// `last_document_total_lines`，避免随后同帧的 metrics 同步重复补偿增长。
    pub fn pin_rebuilt_document_to_anchor(&mut self, total_lines: usize, anchor_line: usize) {
        let max_offset = total_lines.saturating_sub(self.last_visible_height);
        self.scroll_offset = max_offset.saturating_sub(anchor_line).min(max_offset);
        self.last_document_total_lines = total_lines;
        self.auto_scroll = false;
    }

    /// 同步 document 指标并维护滚动真相。
    ///
    /// 每帧渲染前由 App 根据 Output document 与 layout/live-status 投影调用：
    /// - `visible_height` 直接来自当前 layout，不经 OutputArea 反喂；
    /// - document 增长且 `auto_scroll=false` 时补偿 offset，保持视窗内容固定；
    /// - offset 钳制到当前最大可滚动范围；offset 归零时恢复贴尾。
    pub fn sync_document_metrics(&mut self, total_lines: usize, visible_height: usize) {
        let before_visible_height = self.last_visible_height;
        let before_total_lines = self.last_document_total_lines;
        let before_offset = self.scroll_offset;
        let before_auto_scroll = self.auto_scroll;
        self.last_visible_height = visible_height;
        let growth = if !self.auto_scroll {
            let growth = total_lines.saturating_sub(self.last_document_total_lines);
            self.scroll_offset = self.scroll_offset.saturating_add(growth);
            growth
        } else {
            0
        };
        self.last_document_total_lines = total_lines;

        let max_offset = total_lines.saturating_sub(self.last_visible_height);
        self.scroll_offset = self.scroll_offset.min(max_offset);
        if self.scroll_offset == 0 && self.history_window_tail_offset == 0 {
            self.auto_scroll = true;
        }
        if before_visible_height != self.last_visible_height
            || before_total_lines != self.last_document_total_lines
            || before_offset != self.scroll_offset
            || before_auto_scroll != self.auto_scroll
        {
            crate::tui::log_debug!(
                "tui.output.document_metrics before_total_lines={} after_total_lines={} before_visible_height={} after_visible_height={} growth={} max_offset={} before_offset={} after_offset={} before_auto_scroll={} after_auto_scroll={} render_limit={} source_total_lines={} tail_offset={}",
                before_total_lines,
                self.last_document_total_lines,
                before_visible_height,
                self.last_visible_height,
                growth,
                max_offset,
                before_offset,
                self.scroll_offset,
                before_auto_scroll,
                self.auto_scroll,
                self.render_line_limit,
                self.source_total_lines,
                self.history_window_tail_offset
            );
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

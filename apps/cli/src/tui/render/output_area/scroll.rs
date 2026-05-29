impl super::OutputArea {
    /// 向上滚动指定行数
    pub fn scroll_up(&mut self, amount: usize) {
        self.auto_scroll = false;
        let visible_height = self.last_visible_height;
        let max_offset = self.document.total_lines().saturating_sub(visible_height);
        self.scroll_offset = (self.scroll_offset.saturating_add(amount)).min(max_offset);
        if max_offset == 0 {
            self.scroll_offset = 0;
            self.auto_scroll = true;
        }
    }

    /// 向下滚动指定行数
    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
        if self.scroll_offset == 0 {
            self.auto_scroll = true;
        }
    }

    /// 滚动到底部
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    /// 获取行数
    pub fn line_count(&self) -> usize {
        self.document.total_lines()
    }

    /// 获取当前可见行范围
    #[allow(dead_code)]
    pub fn get_visible_range(&self, visible_height: usize) -> (usize, usize) {
        let total_lines = self.document.total_lines();
        if self.auto_scroll {
            let start = total_lines.saturating_sub(visible_height);
            (start, total_lines)
        } else {
            let max_start = total_lines.saturating_sub(visible_height);
            let start = max_start.saturating_sub(self.scroll_offset);
            let start = start.min(max_start);
            (start, (start + visible_height).min(total_lines))
        }
    }
}

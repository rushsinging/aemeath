impl super::OutputArea {
    /// 清空所有内容
    pub fn clear(&mut self) {
        self.document = Default::default();
        self.reset_runtime_state();
    }

    /// 重置输出区域的运行态临时数据
    pub fn reset_runtime_state(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = true;
        self.last_line_count = 0;
        self.is_selecting = false;
        self.selection_start = None;
        self.selection_end = None;
        self.screen_line_map.clear();
        self.spinner = None;
        self.last_visible_height = 0;
        self.todo_subject_cache.clear();
        self.task_status_lines.clear();
    }

    /// 更新 spinner 下方显示的任务状态行
    pub fn set_task_status(&mut self, lines: Vec<String>) {
        self.task_status_lines = lines;
    }
}

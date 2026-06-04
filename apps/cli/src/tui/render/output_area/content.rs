impl super::OutputArea {
    /// 清空所有内容
    pub fn clear(&mut self) {
        self.document = Default::default();
        self.reset_runtime_state();
    }

    /// 重置输出区域的运行态临时数据
    pub fn reset_runtime_state(&mut self) {
        self.last_line_count = 0;
        self.screen_line_map.clear();
        self.last_visible_height = 0;
        self.todo_subject_cache.clear();
    }
}

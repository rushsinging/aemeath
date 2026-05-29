use super::types::{LineStyle, OutputLine, MAX_LINES};

impl super::OutputArea {
    /// 在指定索引处插入一批行
    pub(crate) fn insert_lines_at(&mut self, idx: usize, lines: Vec<OutputLine>) {
        let n = lines.len();
        if n == 0 {
            return;
        }
        let idx = idx.min(self.lines.len());
        for (offset, line) in lines.into_iter().enumerate() {
            self.lines.insert(idx + offset, line);
        }
        self.document = Default::default();
        if !self.auto_scroll {
            self.scroll_offset += n;
        }
        while self.lines.len() > MAX_LINES {
            self.lines.pop_front();
            if self.scroll_offset > 0 {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
        }
    }

    /// 添加一行，超过终端宽度时自动换行
    pub fn push_line(&mut self, line: OutputLine) {
        if self.lines.len() >= MAX_LINES {
            self.lines.pop_front();
            if self.scroll_offset > 0 {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
        }
        self.document = Default::default();
        self.lines.push_back(line);
        if !self.auto_scroll {
            self.scroll_offset += 1;
        }
    }


    /// 添加带有随机烹饪动词和耗时的"完成"消息
    pub fn push_done(&mut self, elapsed: std::time::Duration) {
        let verbs = [
            "Sautéed",
            "Baked",
            "Grilled",
            "Simmered",
            "Roasted",
            "Brewed",
            "Toasted",
            "Stewed",
            "Marinated",
            "Charred",
            "Poached",
            "Steamed",
            "Smoked",
            "Brûléed",
            "Flambéed",
            "Fermented",
            "Pickled",
            "Cured",
            "Seared",
            "Blanched",
        ];
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let idx = COUNTER.fetch_add(1, Ordering::Relaxed) % verbs.len();
        let verb = verbs[idx];

        let secs = elapsed.as_secs();
        let duration = if secs >= 60 {
            format!("{}m {}s", secs / 60, secs % 60)
        } else {
            format!("{}s", secs)
        };

        self.push_line(OutputLine {
            content: format!("✻ {verb} for {duration}"),
            style: LineStyle::System,
            ..Default::default()
        });
        self.push_line(OutputLine {
            content: String::new(),
            style: LineStyle::System,
            ..Default::default()
        });
    }

    /// 清空所有内容
    pub fn clear(&mut self) {
        self.lines.clear();
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

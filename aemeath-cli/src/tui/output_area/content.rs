use super::types::{LineStyle, OutputLine, MAX_LINES};

impl super::OutputArea {
    /// 在指定索引处插入一批行
    pub(super) fn insert_lines_at(&mut self, idx: usize, lines: Vec<OutputLine>) {
        let n = lines.len();
        if n == 0 {
            return;
        }
        let idx = idx.min(self.lines.len());
        for (offset, line) in lines.into_iter().enumerate() {
            self.lines.insert(idx + offset, line);
        }
        if let Some(start) = self.streaming_start {
            if start >= idx {
                self.streaming_start = Some(start + n);
            }
        }
        if !self.auto_scroll {
            self.scroll_offset += n;
        }
        while self.lines.len() > MAX_LINES {
            self.lines.pop_front();
            if self.scroll_offset > 0 {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            if let Some(start) = self.streaming_start {
                self.streaming_start = Some(start.saturating_sub(1));
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
        self.lines.push_back(line);
        if !self.auto_scroll {
            self.scroll_offset += 1;
        }
    }

    /// 添加用户消息
    pub fn push_user_message(&mut self, text: &str) {
        for (i, line) in text.lines().enumerate() {
            let prefix = if i == 0 { "> " } else { "  " };
            self.push_line(OutputLine {
                content: format!("{}{}", prefix, line),
                style: LineStyle::User,
                ..Default::default()
            });
            if self.streaming_start.is_some() {
                self.queued_line_count += 1;
            }
        }
        if text.is_empty() || text.ends_with('\n') {
            self.push_line(OutputLine {
                content: String::new(),
                style: LineStyle::User,
                ..Default::default()
            });
            if self.streaming_start.is_some() {
                self.queued_line_count += 1;
            }
        }
        // 始终将用户刚提交的输入滚动到可视区域，即使之前手动向上滚动过
        self.scroll_to_bottom();
    }

    /// 添加错误消息
    pub fn push_error(&mut self, error: &str) {
        self.finish_streaming();
        self.push_line(OutputLine {
            content: format!("Error: {}", error),
            style: LineStyle::Error,
            ..Default::default()
        });
    }

    /// 添加取消消息
    pub fn push_cancelled(&mut self) {
        self.finish_streaming();
        self.push_line(OutputLine {
            content: "Cancelled".to_string(),
            style: LineStyle::Error,
            ..Default::default()
        });
    }

    /// 添加系统消息
    pub fn push_system(&mut self, msg: &str) {
        self.finish_streaming();
        if msg.is_empty() {
            self.push_line(OutputLine {
                content: String::new(),
                style: LineStyle::System,
                ..Default::default()
            });
            return;
        }
        for line in msg.lines() {
            self.push_line(OutputLine {
                content: line.to_string(),
                style: LineStyle::System,
                ..Default::default()
            });
        }
    }

    /// 添加带有随机烹饪动词和耗时的"完成"消息
    pub fn push_done(&mut self, elapsed: std::time::Duration) {
        let verbs = [
            "Sautéed", "Baked", "Grilled", "Simmered", "Roasted",
            "Brewed", "Toasted", "Stewed", "Marinated", "Charred",
            "Poached", "Steamed", "Smoked", "Brûléed", "Flambéed",
            "Fermented", "Pickled", "Cured", "Seared", "Blanched",
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
        self.scroll_offset = 0;
        self.auto_scroll = true;
        self.streaming_buffer.clear();
        self.streaming_start = None;
        self.synthetic_think_open = false;
        self.queued_line_count = 0;
        self.is_selecting = false;
        self.selection_start = None;
        self.selection_end = None;
        self.screen_line_map.clear();
        self.spinner = None;
        self.todo_subject_cache.clear();
        self.task_status_lines.clear();
    }

    /// 更新 spinner 下方显示的任务状态行
    pub fn set_task_status(&mut self, lines: Vec<String>) {
        self.task_status_lines = lines;
    }
}

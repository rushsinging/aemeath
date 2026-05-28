use super::types::{LineStyle, OutputLine};
use sdk::{ByteIdx, StrSlice};

/// 思考块的开/闭标记。这两个常量是 ASCII 字符串，长度计算是字节安全的。
/// **不要** 替换为多字节中文/emoji 字符串——`do_rerender` 里的偏移依赖
/// 它们的字节长度，否则会 UTF-8 边界 panic（曾经因为误翻译为 `思路` /
/// `</think` 触发过 `byte index 7 is not a char boundary`）。
const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

impl super::OutputArea {
    /// 判断当前是否在未闭合的思考块内
    pub(super) fn has_unclosed_think(&self) -> bool {
        let opens = self.streaming_buffer.matches(THINK_OPEN).count();
        let closes = self.streaming_buffer.matches(THINK_CLOSE).count();
        opens > closes
    }

    /// 将思考文本追加到流式块
    pub fn append_thinking_text(&mut self, text: &str) {
        if text.contains(THINK_OPEN) || text.contains(THINK_CLOSE) {
            self.streaming_buffer.push_str(text);
        } else {
            if !self.has_unclosed_think() {
                self.streaming_buffer.push_str(THINK_OPEN);
                self.synthetic_think_open = true;
            }
            self.streaming_buffer.push_str(text);
        }
        self.do_rerender();
    }

    /// 将文本追加到流式助手块
    pub fn append_assistant_text(&mut self, text: &str) {
        if self.synthetic_think_open && self.has_unclosed_think() {
            self.streaming_buffer.push_str(THINK_CLOSE);
            self.streaming_buffer.push('\n');
            self.synthetic_think_open = false;
        }
        self.streaming_buffer.push_str(text);
        self.do_rerender();
    }

    /// 流式块的核心重绘逻辑
    pub(super) fn do_rerender(&mut self) {
        if self.streaming_start.is_none() {
            self.streaming_start = Some(self.lines.len());
        }

        let start_idx = self.streaming_start.unwrap_or(0);
        let old_line_count = self.lines.len();

        // 保存排队消息行（流式过程中添加的）
        let queued_lines: Vec<OutputLine> = (0..self.queued_line_count)
            .filter_map(|_| self.lines.pop_back())
            .collect();
        self.queued_line_count = 0;

        // 移除流式块中的所有行
        while self.lines.len() > start_idx {
            self.lines.pop_back();
        }

        // 将缓冲区解析为片段：思考内容 vs 普通内容
        let buf = &self.streaming_buffer;
        let mut pos = ByteIdx::ZERO;
        let mut segments: Vec<(&str, bool)> = Vec::new();

        while pos.as_usize() < buf.len() {
            if let Some(think_start) = buf[pos.as_usize()..].find(THINK_OPEN) {
                let abs_start = ByteIdx::new(pos.as_usize() + think_start);
                if abs_start > pos {
                    segments.push((buf.bslice(pos..abs_start), false));
                }
                // 使用 ByteIdx::after_str 防止字面量长度硬编码
                let content_start = abs_start.after_str(THINK_OPEN);
                if let Some(think_end) = buf[content_start.as_usize()..].find(THINK_CLOSE) {
                    let abs_end = ByteIdx::new(content_start.as_usize() + think_end);
                    segments.push((buf.bslice(content_start..abs_end), true));
                    pos = abs_end.after_str(THINK_CLOSE);
                } else {
                    segments.push((buf.bslice_from(content_start), true));
                    pos = ByteIdx::end_of(buf);
                }
            } else {
                segments.push((buf.bslice_from(pos), false));
                pos = ByteIdx::end_of(buf);
            }
        }

        // 渲染片段
        for (segment, is_thinking) in &segments {
            let style = if *is_thinking {
                LineStyle::Thinking
            } else {
                LineStyle::Assistant
            };
            let prefix = if *is_thinking { "💭 " } else { "" };
            for text_line in segment.lines() {
                let display_line = if *is_thinking && !text_line.is_empty() {
                    format!("{prefix}{text_line}")
                } else {
                    text_line.to_string()
                };
                self.lines.push_back(OutputLine {
                    content: display_line,
                    style,
                    tool_id: None,
                    spans: None,
                });
            }
        }

        // 如果缓冲区以换行符结尾，添加空行
        if self.streaming_buffer.ends_with('\n') {
            self.lines.push_back(OutputLine {
                content: String::new(),
                style: LineStyle::Assistant,
                tool_id: None,
                spans: None,
            });
        }

        // 恢复排队消息行
        for line in queued_lines.into_iter().rev() {
            self.lines.push_back(line);
            self.queued_line_count += 1;
        }

        self.rendered_cache
            .line_cache
            .content_changed(self.lines.len());

        // 调整滚动偏移
        if !self.auto_scroll {
            let new_line_count = self.lines.len();
            if new_line_count > old_line_count {
                self.scroll_offset += new_line_count - old_line_count;
            } else if new_line_count < old_line_count {
                self.scroll_offset = self
                    .scroll_offset
                    .saturating_sub(old_line_count - new_line_count);
            }
        }
    }

    /// 结束当前流式块
    pub fn finish_streaming(&mut self) {
        self.streaming_buffer.clear();
        self.streaming_start = None;
        self.synthetic_think_open = false;
        self.queued_line_count = 0;
    }
}

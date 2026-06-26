use super::copied_text::CopiedTextSpan;
use super::image_span::ImageSpan;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputDocument {
    pub buffer: String,
    pub cursor: usize,
    pub copied_text_spans: Vec<CopiedTextSpan>,
    pub image_spans: Vec<ImageSpan>,
}

impl InputDocument {
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn insert_text(&mut self, text: &str) {
        let cursor = self.cursor.min(self.buffer.len());
        let cursor = clamp_to_char_boundary(&self.buffer, cursor);
        self.buffer.insert_str(cursor, text);
        self.shift_spans_for_insert(cursor, text.len());
        self.cursor = cursor + text.len();
    }

    pub fn insert_pasted_text(&mut self, text: &str) {
        if should_collapse_paste(text) {
            self.insert_copied_text(text);
        } else {
            self.insert_text(text);
        }
    }

    /// 插入图片占位符 `[Image #N]` 并记录 span。序号固定不重排。
    pub fn insert_image(&mut self, image: sdk::ClipboardImageView) {
        let index = self.image_spans.len() + 1;
        let placeholder = format!("[Image #{index}]");
        let cursor = clamp_to_char_boundary(&self.buffer, self.cursor.min(self.buffer.len()));
        self.buffer.insert_str(cursor, &placeholder);
        self.shift_spans_for_insert(cursor, placeholder.len());
        let end = cursor + placeholder.len();
        self.image_spans
            .push(ImageSpan::new(index, image, cursor, end));
        self.cursor = end;
    }

    pub fn replace_text(&mut self, text: String) {
        self.buffer = text;
        self.cursor = self.buffer.len();
        self.copied_text_spans.clear();
        self.image_spans.clear();
    }

    pub fn move_cursor(&mut self, cursor: usize) {
        self.cursor = clamp_to_char_boundary(&self.buffer, cursor.min(self.buffer.len()));
    }

    /// 用字符索引（char index）设置光标，自动转为字节位置
    /// textarea 的光标列号是字符索引，模型需要字节位置
    pub fn set_cursor_col(&mut self, col: usize) {
        let byte_pos = self
            .buffer
            .char_indices()
            .nth(col)
            .map(|(idx, _)| idx)
            .unwrap_or(self.buffer.len());
        self.move_cursor(byte_pos);
    }

    pub fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut previous = 0;
        for (idx, _) in self.buffer.char_indices() {
            if idx >= self.cursor {
                break;
            }
            previous = idx;
        }
        self.cursor = previous;
    }

    pub fn move_right(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let next = self.buffer[self.cursor..]
            .chars()
            .next()
            .map(|ch| self.cursor + ch.len_utf8())
            .unwrap_or(self.buffer.len());
        self.cursor = next.min(self.buffer.len());
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    pub fn delete_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((start, end)) = self.atomic_span_for_backward_delete() {
            self.delete_range(start, end);
            return;
        }
        let old_cursor = self.cursor;
        self.move_left();
        self.delete_range(self.cursor, old_cursor);
    }

    pub fn delete_word_before_cursor(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((start, end)) = self.atomic_span_for_backward_delete() {
            self.delete_range(start, end);
            return;
        }
        let end = clamp_to_char_boundary(&self.buffer, self.cursor);
        let prefix = &self.buffer[..end];
        let trimmed_end = prefix.trim_end_matches(char::is_whitespace).len();
        let mut start = trimmed_end;
        for (idx, ch) in prefix[..trimmed_end].char_indices().rev() {
            if ch.is_whitespace() {
                start = idx + ch.len_utf8();
                break;
            }
            start = idx;
        }
        self.delete_range(start, end);
    }

    pub fn delete_forward(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let start = self.cursor;
        self.move_right();
        let end = self.cursor;
        self.delete_range(start, end);
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.copied_text_spans.clear();
        self.image_spans.clear();
    }

    pub fn display_text(&self) -> String {
        self.buffer.clone()
    }

    pub fn expand_copied_text(&self) -> String {
        if self.copied_text_spans.is_empty() {
            return self.buffer.clone();
        }
        let mut expanded = String::new();
        let mut cursor = 0;
        let mut spans = self.copied_text_spans.clone();
        spans.sort_by_key(|span| span.start);
        for span in spans {
            if span.start > cursor {
                expanded.push_str(&self.buffer[cursor..span.start]);
            }
            expanded.push_str(&span.original);
            cursor = span.end;
        }
        if cursor < self.buffer.len() {
            expanded.push_str(&self.buffer[cursor..]);
        }
        expanded
    }

    /// 提交输入框文本。把所有 span（copied text + image placeholder）按原始 buffer 位置
    /// 还原成完整文本，结果含 image placeholder（`[Image #N]`），与 TUI 端
    /// `enqueue_submission_echo` 用的 `display_text` 一致。
    ///
    /// #507 修复：**保留** image placeholder（而非剔除为空），让 runtime 端
    /// `Message::user_with_images` 能按 text 中 `[Image #N]` 占位符穿插拆块，
    /// 最终 `text_content()` 还原出 "看图[Image #1]" 而非 "[Image #1]看图"。
    ///
    /// 基于**原始 buffer** 的位置统一遍历两类 span，避免先 expand 再定位导致错位。
    pub fn submit_text(&self) -> String {
        if self.copied_text_spans.is_empty() && self.image_spans.is_empty() {
            return self.buffer.trim().to_string();
        }
        // 收集所有替换区间：copied text → 原文；image → 占位符 `[Image #N]`（保留）
        let mut replacements: Vec<(usize, usize, String)> = self
            .copied_text_spans
            .iter()
            .map(|span| (span.start, span.end, span.original.to_string()))
            .collect();
        replacements.extend(
            self.image_spans
                .iter()
                .map(|span| (span.start, span.end, span.placeholder())),
        );
        // 按 start 排序，正向构建结果，位置始终基于原始 buffer
        replacements.sort_by_key(|(start, _, _)| *start);
        let mut result = String::new();
        let mut cursor = 0;
        for (start, end, repl) in &replacements {
            if *start > cursor {
                result.push_str(&self.buffer[cursor..*start]);
            }
            result.push_str(repl);
            cursor = *end;
        }
        if cursor < self.buffer.len() {
            result.push_str(&self.buffer[cursor..]);
        }
        result.trim().to_string()
    }

    /// 按文档中出现顺序取出全部图片数据。
    /// 按文档中出现顺序取出全部图片，附带 placeholder id（#fix-tui-image-input-output）。
    ///
    /// 返回 `(placeholder, image)` 配对，placeholder 为 TUI 端
    /// `ImageSpan::placeholder()` 生成的字符串（如 `"[Image #1]"`），与 `text` 中
    /// 占位符一一对应——runtime 端按 text 中 `[Image #N]` 出现顺序穿插拆块。
    pub fn drain_images(&mut self) -> Vec<(String, sdk::ChatInputImage)> {
        let mut spans = std::mem::take(&mut self.image_spans);
        spans.sort_by_key(|span| span.start);
        spans
            .into_iter()
            .map(|span| {
                let placeholder = span.placeholder();
                let image: sdk::ChatInputImage = span.image.into();
                (placeholder, image)
            })
            .collect()
    }

    /// 移除所有图片占位符，保留其余文本。
    pub fn remove_all_images(&mut self) {
        // 按 start 倒序移除，避免偏移重算
        let mut spans: Vec<ImageSpan> = std::mem::take(&mut self.image_spans);
        spans.sort_by_key(|span| std::cmp::Reverse(span.start));
        for span in &spans {
            let placeholder = span.placeholder();
            if self.buffer.get(span.start..span.end) == Some(placeholder.as_str()) {
                self.buffer.replace_range(span.start..span.end, "");
            }
        }
        // 重新计算光标位置（clamp 到合法范围）
        self.cursor = clamp_to_char_boundary(&self.buffer, self.cursor.min(self.buffer.len()));
        // 清理已删除的 spans 中可能与 copied_text_spans 重叠的情况不需要处理，
        // 因为 image_spans 已经被 take 掉了
    }

    /// 光标所在行号（从 0 开始）
    pub fn cursor_row(&self) -> usize {
        self.buffer[..self.cursor].matches('\n').count()
    }

    /// 光标在当前行中的字节偏移（不含前面的换行符）
    pub fn cursor_col_byte_offset(&self) -> usize {
        let before_cursor = &self.buffer[..self.cursor];
        if let Some(pos) = before_cursor.rfind('\n') {
            self.cursor - pos - 1
        } else {
            self.cursor
        }
    }

    /// 光标在当前行中的字符列号（从 0 开始）
    pub fn cursor_col(&self) -> usize {
        let before_cursor = &self.buffer[..self.cursor];
        if let Some(pos) = before_cursor.rfind('\n') {
            self.buffer[pos + 1..self.cursor].chars().count()
        } else {
            before_cursor.chars().count()
        }
    }

    /// 总行数（空 buffer 为 1）
    pub fn line_count(&self) -> usize {
        if self.buffer.is_empty() {
            return 1;
        }
        self.buffer.matches('\n').count() + 1
    }

    /// 光标是否在第一行
    pub fn is_cursor_at_first_line(&self) -> bool {
        !self.buffer[..self.cursor].contains('\n')
    }

    /// 光标是否在最后一行
    pub fn is_cursor_at_last_line(&self) -> bool {
        !self.buffer[self.cursor..].contains('\n')
    }

    /// 将光标移到上一行，保持列位置。已在第一行则不移动。
    pub fn move_up(&mut self) {
        if self.is_cursor_at_first_line() {
            return;
        }
        let col = self.cursor_col();
        // 找到当前行的开头（上一个 \n 的下一个位置）
        let line_start = self.buffer[..self.cursor]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        // 上一行的开头
        let prev_line_start = self.buffer[..line_start.saturating_sub(1)]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(0);
        let prev_line_end = line_start.saturating_sub(1); // 当前行开头的 \n 位置
                                                          // SAFETY: prev_line_start/prev_line_end 由 \n 搜索推算，\n 是单字节 ASCII，
                                                          // 因此偏移一定在 char 边界上。
        let prev_line = self
            .buffer
            .get(prev_line_start..prev_line_end)
            .unwrap_or("");
        let new_col = col.min(prev_line.chars().count());
        let new_cursor = prev_line_start
            + prev_line
                .char_indices()
                .nth(new_col)
                .map(|(idx, _)| idx)
                .unwrap_or(prev_line.len());
        self.cursor = new_cursor;
    }

    /// 将光标移到下一行，保持列位置。已在最后一行则不移动。
    pub fn move_down(&mut self) {
        if self.is_cursor_at_last_line() {
            return;
        }
        let col = self.cursor_col();
        // 找到下一行的开头
        let next_newline = self.buffer[self.cursor..]
            .find('\n')
            .map(|pos| self.cursor + pos + 1);
        let Some(next_line_start) = next_newline else {
            return;
        };
        // 下一行的结尾
        let next_line_end = self.buffer[next_line_start..]
            .find('\n')
            .map(|pos| next_line_start + pos)
            .unwrap_or(self.buffer.len());
        // SAFETY: next_line_start/next_line_end 由 \n 搜索推算，\n 是单字节 ASCII，
        // 因此偏移一定在 char 边界上。
        let next_line = self
            .buffer
            .get(next_line_start..next_line_end)
            .unwrap_or("");
        let new_col = col.min(next_line.chars().count());
        let new_cursor = next_line_start
            + next_line
                .char_indices()
                .nth(new_col)
                .map(|(idx, _)| idx)
                .unwrap_or(next_line.len());
        self.cursor = new_cursor;
    }

    fn insert_copied_text(&mut self, original: &str) {
        let trimmed = original.trim();
        let line_count = non_empty_line_count(trimmed);
        let placeholder = format!("[Copied {} lines]", line_count);
        let cursor = clamp_to_char_boundary(&self.buffer, self.cursor.min(self.buffer.len()));
        self.buffer.insert_str(cursor, &placeholder);
        self.shift_spans_for_insert(cursor, placeholder.len());
        let end = cursor + placeholder.len();
        self.copied_text_spans
            .push(CopiedTextSpan::new(placeholder, trimmed, cursor, end));
        self.cursor = end;
    }

    /// 光标紧邻 span 末尾时，返回该 span 的区间，实现原子删除。
    fn atomic_span_for_backward_delete(&self) -> Option<(usize, usize)> {
        self.copied_text_spans
            .iter()
            .find(|span| self.cursor > span.start && self.cursor <= span.end)
            .map(|span| (span.start, span.end))
            .or_else(|| {
                self.image_spans
                    .iter()
                    .find(|span| self.cursor > span.start && self.cursor <= span.end)
                    .map(|span| (span.start, span.end))
            })
    }

    fn delete_range(&mut self, start: usize, end: usize) {
        let start = clamp_to_char_boundary(&self.buffer, start.min(self.buffer.len()));
        let end = clamp_to_char_boundary(&self.buffer, end.min(self.buffer.len()));
        if start >= end {
            self.cursor = start;
            return;
        }
        self.buffer.drain(start..end);
        let deleted_len = end - start;
        self.copied_text_spans
            .retain(|span| !(span.start >= start && span.end <= end));
        self.image_spans
            .retain(|span| !(span.start >= start && span.end <= end));
        for span in &mut self.copied_text_spans {
            if span.start >= end {
                span.start -= deleted_len;
                span.end -= deleted_len;
            }
        }
        for span in &mut self.image_spans {
            if span.start >= end {
                span.start -= deleted_len;
                span.end -= deleted_len;
            }
        }
        self.cursor = start;
    }

    fn shift_spans_for_insert(&mut self, start: usize, len: usize) {
        for span in &mut self.copied_text_spans {
            if span.start >= start {
                span.start += len;
                span.end += len;
            }
        }
        for span in &mut self.image_spans {
            if span.start >= start {
                span.start += len;
                span.end += len;
            }
        }
    }
}

fn should_collapse_paste(text: &str) -> bool {
    non_empty_line_count(text.trim()) > 3
}

fn non_empty_line_count(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.lines().filter(|l| !l.trim().is_empty()).count()
}

fn clamp_to_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

#[cfg(test)]
#[path = "document_tests.rs"]
mod document_tests;

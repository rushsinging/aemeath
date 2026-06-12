use super::copied_text::CopiedTextSpan;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputDocument {
    pub buffer: String,
    pub cursor: usize,
    pub copied_text_spans: Vec<CopiedTextSpan>,
    next_copied_text_index: usize,
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

    pub fn replace_text(&mut self, text: String) {
        self.buffer = text;
        self.cursor = self.buffer.len();
        self.copied_text_spans.clear();
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
        if let Some((start, end)) = self.copied_text_span_for_backward_delete() {
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
        if let Some((start, end)) = self.copied_text_span_for_backward_delete() {
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
                                                          // allow unsafe_text_op: prev_line_start/prev_line_end 由 \n 搜索推算，ASCII 边界有效
        let prev_line = &self.buffer[prev_line_start..prev_line_end]; // allow unsafe_text_op
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
        // allow unsafe_text_op: next_line_start/next_line_end 由 \n 搜索推算，ASCII 边界有效
        let next_line = &self.buffer[next_line_start..next_line_end]; // allow unsafe_text_op
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
        self.next_copied_text_index += 1;
        let placeholder = format!("[Copied Text {}]", self.next_copied_text_index);
        let cursor = clamp_to_char_boundary(&self.buffer, self.cursor.min(self.buffer.len()));
        self.buffer.insert_str(cursor, &placeholder);
        self.shift_spans_for_insert(cursor, placeholder.len());
        let end = cursor + placeholder.len();
        self.copied_text_spans
            .push(CopiedTextSpan::new(placeholder, original, cursor, end));
        self.cursor = end;
    }

    fn copied_text_span_for_backward_delete(&self) -> Option<(usize, usize)> {
        self.copied_text_spans
            .iter()
            .find(|span| self.cursor > span.start && self.cursor <= span.end)
            .map(|span| (span.start, span.end))
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
        for span in &mut self.copied_text_spans {
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
    }
}

fn should_collapse_paste(text: &str) -> bool {
    text.matches('\n').count() >= 2
}

fn clamp_to_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_text_advances_cursor() {
        let mut doc = InputDocument::default();
        doc.insert_text("abc");
        assert_eq!(doc.buffer, "abc");
        assert_eq!(doc.cursor, 3);
    }

    #[test]
    fn test_move_cursor_clamps_to_buffer() {
        let mut doc = InputDocument::default();
        doc.insert_text("abc");
        doc.move_cursor(99);
        assert_eq!(doc.cursor, 3);
        doc.move_cursor(0);
        assert_eq!(doc.cursor, 0);
    }

    #[test]
    fn test_delete_backward_at_start_is_noop() {
        let mut doc = InputDocument::default();
        doc.delete_backward();
        assert_eq!(doc.buffer, "");
        assert_eq!(doc.cursor, 0);
    }

    // 回归测试：Bug #77 — @ 补全后按空格回退删除字符
    //
    // 场景：用户输入 @src/m 触发补全，Tab 确认后 textarea 变为 @src/main.rs，
    // 但模型未同步（仍是 @src/m）。此时按空格，模型 insert(' ') 生成 @src/m ，
    // TextChanged → set_text 用旧文本覆盖 textarea 的正确内容。
    //
    // 修复：补全确认后执行 clear + insert_text 将模型同步到 textarea 的文本。

    /// 模拟补全后模型未同步 → 按空格 → 旧文本覆盖 textarea 的 bug 场景
    #[test]
    fn test_stale_model_overwrites_textarea_after_completion() {
        // Step 1: 用户输入 @src/m → 模型 = @src/m
        let mut doc = InputDocument::default();
        doc.insert_text("@src/m");
        assert_eq!(doc.buffer, "@src/m");
        assert_eq!(doc.cursor, 6);

        // Step 2: 用户 Tab 补全 → textarea 变为 @src/main.rs
        //         但模型未同步（保持 @src/m），这是 bug 条件
        //         模拟：textarea = @src/main.rs，模型仍是 @src/m

        // Step 3: 用户按空格 → 模型 insert(' ')，产生 @src/m ，
        //         TextChanged + set_text 用 @src/m  覆盖 textarea 的 @src/main.rs
        doc.insert_text(" ");
        // BUG：模型生成 @src/m ，set_text 会覆盖 textarea 上的 @src/main.rs
        assert_eq!(doc.buffer, "@src/m ");
        // 修复前的错误行为验证：buffer 长度 7 而非 @src/main.rs  的 13
        assert_ne!(doc.buffer, "@src/main.rs ");
    }

    /// 修复后：补全确认后模型同步 → 按空格 → 正确追加
    #[test]
    fn test_synced_model_after_completion_preserves_text() {
        let mut doc = InputDocument::default();
        // 用户输入 @src/m
        doc.insert_text("@src/m");
        assert_eq!(doc.buffer, "@src/m");

        // 补全确认后 textarea 变为 @src/main.rs，模型同步：
        doc.clear();
        doc.insert_text("@src/main.rs");
        assert_eq!(doc.buffer, "@src/main.rs");
        assert_eq!(doc.cursor, 12);

        // 按空格：模型在正确位置插入空格
        doc.insert_text(" ");
        assert_eq!(doc.buffer, "@src/main.rs ");
        assert_eq!(doc.cursor, 13);
    }

    /// CJK 路径补全后模型同步 + 按空格
    #[test]
    fn test_synced_model_after_cjk_completion_preserves_text() {
        let mut doc = InputDocument::default();
        // 用户输入 @中文/路径
        doc.insert_text("@中文/路径");
        assert_eq!(doc.buffer, "@中文/路径");

        // 补全确认后 textarea 变为 @中文/路径/目标.rs，模型同步：
        doc.clear();
        doc.insert_text("@中文/路径/目标.rs");
        assert_eq!(doc.buffer, "@中文/路径/目标.rs");
        assert_eq!(doc.cursor, "@中文/路径/目标.rs".len());

        // 按空格
        doc.insert_text(" ");
        assert_eq!(doc.buffer, "@中文/路径/目标.rs ");
        assert_eq!(doc.cursor, "@中文/路径/目标.rs ".len());
    }

    // 回归测试：Bug #78 — 粘贴后按空格清空粘贴内容
    //
    // 场景：用户粘贴文本，input_area.input(ch) 直接修改 textarea 但未同步模型。
    // 按空格时模型 insert(' ') 产生旧文本+空格 → set_text 覆盖 textarea 中的粘贴内容。
    //
    // 修复：粘贴循环后执行 clear + insert_text 将模型同步到 textarea 的文本。

    /// 粘贴后模型同步 → 按空格 → 正确追加
    #[test]
    fn test_synced_model_after_paste_preserves_text() {
        let mut doc = InputDocument::default();
        // 粘贴前状态（模型）
        doc.insert_text("before ");
        assert_eq!(doc.buffer, "before ");

        // 用户粘贴 "hello world" → textarea = "before hello world"
        // 修复：粘贴后同步模型
        doc.clear();
        doc.insert_text("before hello world");
        assert_eq!(doc.buffer, "before hello world");

        // 按空格
        doc.insert_text(" ");
        assert_eq!(doc.buffer, "before hello world ");
        assert_eq!(doc.cursor, "before hello world ".len());
    }

    /// 粘贴 Cjk 文本后模型同步 + 按空格
    #[test]
    fn test_synced_model_after_cjk_paste_preserves_text() {
        let mut doc = InputDocument::default();
        // 粘贴前
        doc.insert_text("前置");
        assert_eq!(doc.buffer, "前置");

        // 用户粘贴 "你好世界" → textarea = "前置你好世界"
        doc.clear();
        doc.insert_text("前置你好世界");
        assert_eq!(doc.buffer, "前置你好世界");

        // 按空格
        doc.insert_text(" ");
        assert_eq!(doc.buffer, "前置你好世界 ");
    }

    // 回归测试：Bug #79 — Option+Enter 换行后继续输入回到上一行
    //
    // 场景：用户 Option+Enter 在 textarea 中插入换行，但 textarea.enter(true)
    // 直接修改了 textarea 未同步模型。模型仍是单行旧文本 — 继续输入时
    // model.insert → TextChanged → set_text 用旧文本覆盖 textarea，换行丢失。
    //
    // 修复：enter(true) 后执行 clear + insert_text 同步模型。

    /// 换行后模型同步 → 继续输入 → 保持在第二行
    #[test]
    fn test_synced_model_after_newline_preserves_text() {
        let mut doc = InputDocument::default();
        // 用户在第一行输入
        doc.insert_text("第一行文字");
        assert_eq!(doc.buffer, "第一行文字");

        // 用户 Option+Enter 换行 → textarea = "第一行文字\n"
        // 修复：换行后同步模型
        doc.clear();
        doc.insert_text("第一行文字\n");
        assert_eq!(doc.buffer, "第一行文字\n");

        // 用户在第二行继续输入
        doc.insert_text("第二行");
        assert_eq!(doc.buffer, "第一行文字\n第二行");
    }

    /// Cjk 换行后模型同步 → 继续输入 → 正确
    #[test]
    fn test_synced_model_after_newline_cjk_preserves_text() {
        let mut doc = InputDocument::default();
        doc.insert_text("中文内容");
        assert_eq!(doc.buffer, "中文内容");

        // Option+Enter 换行
        doc.clear();
        doc.insert_text("中文内容\n");
        assert_eq!(doc.buffer, "中文内容\n");

        // 继续输入
        doc.insert_text("继续输入");
        assert_eq!(doc.buffer, "中文内容\n继续输入");
    }

    // 回归测试：光标移动未同步模型 cursor — InsertChar 以旧位置插入
    //
    // 场景：Ctrl+A/E/Left/Right/End 移动 textarea 光标，但模型 cursor 未更新。
    // 后续 InsertChar 以模型旧光标位置插入，造成错位。
    // 这在 CJK 文本中尤为明显（字符索引 ≠ 字节位置）。
    //
    // 修复：set_cursor_col 将 textarea 的字符索引光标转换为模型字节位置

    #[test]
    fn test_set_cursor_col_sets_correct_byte_position() {
        let mut doc = InputDocument::default();
        doc.insert_text("abc你好def");
        assert_eq!(doc.buffer, "abc你好def");
        assert_eq!(doc.cursor, "abc你好def".len());

        // textarea 光标移到字符索引 3（'你' 的位置）
        doc.set_cursor_col(3);
        // 字节位置：'a'(1) + 'b'(1) + 'c'(1) = 3
        assert_eq!(doc.cursor, 3);

        // textarea 光标移到字符索引 4（'好' 的位置）
        doc.set_cursor_col(4);
        // 字节位置：3 + '你'(3) = 6
        assert_eq!(doc.cursor, 6);
    }

    #[test]
    fn test_insert_at_synced_cursor_after_move() {
        let mut doc = InputDocument::default();
        doc.insert_text("abc你好def");
        assert_eq!(doc.cursor, "abc你好def".len());

        // 模拟光标移动到 start (Ctrl+A)
        doc.set_cursor_col(0);
        assert_eq!(doc.cursor, 0);

        // 在行首插入
        doc.insert_text(">>");
        assert_eq!(doc.buffer, ">>abc你好def");
        assert_eq!(doc.cursor, ">>".len());
    }

    #[test]
    fn test_insert_at_cjk_boundary_after_cursor_sync() {
        let mut doc = InputDocument::default();
        doc.insert_text("你好世界");

        // 光标移到字符索引 2 (在 '你好' 之后，'世界' 之前)
        doc.set_cursor_col(2);
        assert_eq!(doc.cursor, "你好".len());

        // 插入
        doc.insert_text("的");
        assert_eq!(doc.buffer, "你好的世界");
        assert_eq!(doc.cursor, "你好的".len());
    }

    // 回归测试：Bug #99 — 多行输入时光标上下移动
    //
    // 场景：用户在多行文本中按 ↑/↓，期望光标在行间移动，
    // 但旧逻辑始终触发历史翻看。修复后 InputDocument 支持
    // move_up/move_down，在边界行时由 InputModel 切换到历史导航。

    #[test]
    fn test_move_up_goes_to_previous_line() {
        let mut doc = InputDocument::default();
        doc.insert_text("第一行\n第二行\n第三行");
        // 光标在第三行末尾
        assert_eq!(doc.cursor_row(), 2);
        doc.move_up();
        assert_eq!(doc.cursor_row(), 1, "应移到第二行");
        doc.move_up();
        assert_eq!(doc.cursor_row(), 0, "应移到第一行");
        // 已在第一行，再次 move_up 不动
        doc.move_up();
        assert_eq!(doc.cursor_row(), 0, "已在第一行，不应移动");
    }

    #[test]
    fn test_move_down_goes_to_next_line() {
        let mut doc = InputDocument::default();
        doc.insert_text("第一行\n第二行\n第三行");
        doc.move_cursor(0); // 光标移到第一行开头
        assert_eq!(doc.cursor_row(), 0);
        doc.move_down();
        assert_eq!(doc.cursor_row(), 1, "应移到第二行");
        doc.move_down();
        assert_eq!(doc.cursor_row(), 2, "应移到第三行");
        // 已在最后一行，再次 move_down 不动
        doc.move_down();
        assert_eq!(doc.cursor_row(), 2, "已在最后一行，不应移动");
    }

    #[test]
    fn test_move_up_down_preserves_column_position() {
        let mut doc = InputDocument::default();
        doc.insert_text("abcde\nxyz");
        // 光标在第二行末尾（xyz 之后）
        // 移到第二行的 'y' 位置（字符列 1）
        doc.set_cursor_col("abcde\n".len() + "x".len());
        assert_eq!(doc.cursor_col(), 1);
        doc.move_up();
        assert_eq!(doc.cursor_row(), 0, "应移到第一行");
        assert_eq!(doc.cursor_col(), 1, "列应保持为 1（即 'b' 的位置）");
        // 验证光标在 'b' 位置
        assert_eq!(doc.cursor, 1);
    }

    #[test]
    fn test_move_up_down_clamps_shorter_line() {
        let mut doc = InputDocument::default();
        doc.insert_text("ab\nxyzw");
        // 光标在第二行的 'w' 之后（列 4）
        doc.move_up();
        assert_eq!(doc.cursor_row(), 0, "应移到第一行");
        // 第一行只有 2 个字符，列 4 应 clamp 到 2
        assert_eq!(doc.cursor_col(), 2, "列应 clamp 到第一行长 2");
        assert_eq!(doc.cursor, 2, "光标应在第一行末尾");
    }

    #[test]
    fn test_move_up_down_single_line_is_noop() {
        let mut doc = InputDocument::default();
        doc.insert_text("单行文本");
        let cursor_before = doc.cursor;
        doc.move_up();
        assert_eq!(doc.cursor, cursor_before, "单行时 move_up 不动");
        doc.move_down();
        assert_eq!(doc.cursor, cursor_before, "单行时 move_down 不动");
    }

    #[test]
    fn test_cursor_row_col_with_cjk() {
        let mut doc = InputDocument::default();
        doc.insert_text("你好\n世界");
        // 光标在末尾（第二行'世界'之后）
        assert_eq!(doc.cursor_row(), 1);
        assert_eq!(doc.cursor_col(), 2, "第二行有 2 个字符");
        doc.move_cursor(0);
        assert_eq!(doc.cursor_row(), 0);
        assert_eq!(doc.cursor_col(), 0);
    }

    #[test]
    fn test_move_up_down_with_cjk() {
        let mut doc = InputDocument::default();
        doc.insert_text("你好世界\nabc\n测试");
        // 光标在第三行末尾
        assert_eq!(doc.cursor_row(), 2);
        doc.move_up();
        assert_eq!(doc.cursor_row(), 1);
        doc.move_up();
        assert_eq!(doc.cursor_row(), 0);
        doc.move_down();
        assert_eq!(doc.cursor_row(), 1);
    }

    #[test]
    fn test_is_cursor_at_first_last_line() {
        let mut doc = InputDocument::default();
        doc.insert_text("第一行\n第二行\n第三行");
        assert!(doc.is_cursor_at_last_line());
        assert!(!doc.is_cursor_at_first_line());
        doc.move_cursor(0);
        assert!(doc.is_cursor_at_first_line());
        assert!(!doc.is_cursor_at_last_line());
        // 移到第二行开头（第一行末尾的 \n 之后）
        let second_line_start = "第一行\n".len();
        doc.move_cursor(second_line_start);
        assert!(!doc.is_cursor_at_first_line());
        assert!(!doc.is_cursor_at_last_line());
    }
}

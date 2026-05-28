#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputDocument {
    pub buffer: String,
    pub cursor: usize,
    pub selection: Option<InputSelection>,
}

impl InputDocument {
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn insert_text(&mut self, text: &str) {
        let cursor = self.cursor.min(self.buffer.len());
        let cursor = clamp_to_char_boundary(&self.buffer, cursor);
        self.buffer.insert_str(cursor, text);
        self.cursor = cursor + text.len();
        self.selection = None;
    }

    pub fn replace_text(&mut self, text: String) {
        self.buffer = text;
        self.cursor = self.buffer.len();
        self.selection = None;
    }

    pub fn move_cursor(&mut self, cursor: usize) {
        self.cursor = clamp_to_char_boundary(&self.buffer, cursor.min(self.buffer.len()));
        self.selection = None;
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
        self.selection = None;
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
        self.selection = None;
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
        self.selection = None;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.buffer.len();
        self.selection = None;
    }

    pub fn delete_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let old_cursor = self.cursor;
        self.move_left();
        self.buffer.drain(self.cursor..old_cursor);
        self.selection = None;
    }

    pub fn delete_word_before_cursor(&mut self) {
        if self.cursor == 0 {
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
        self.buffer.drain(start..end);
        self.cursor = start;
        self.selection = None;
    }

    pub fn delete_forward(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let start = self.cursor;
        self.move_right();
        let end = self.cursor;
        self.buffer.drain(start..end);
        self.cursor = start;
        self.selection = None;
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.selection = None;
    }
}

fn clamp_to_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputSelection {
    pub start: usize,
    pub end: usize,
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
}

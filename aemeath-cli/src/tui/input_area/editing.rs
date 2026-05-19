use super::InputArea;
use crate::tui::safe_text::safe_char_slice;
use tui_textarea::CursorMove;
use unicode_width::UnicodeWidthChar;

impl InputArea {
    /// Handle a character input
    pub fn input(&mut self, c: char) {
        self.textarea.insert_char(c);
        self.hide_suggestions(); // Hide suggestions while typing
        self.reset_history_nav(); // Reset history navigation when typing
        self.auto_wrap_current_line();
    }

    /// 当前行超过视觉宽度时，在合适位置插入换行符
    fn auto_wrap_current_line(&mut self) {
        let max_w = self.content_width as usize;
        if max_w < 10 {
            // 宽度太小不折行
            return;
        }
        let (row, col) = self.textarea.cursor();
        let lines = self.textarea.lines();
        if row >= lines.len() {
            return;
        }
        let chars: Vec<char> = lines[row].chars().collect();
        if display_width(&chars) <= max_w {
            return;
        }

        let best_break = find_wrap_break(&chars, max_w);
        if best_break == 0 {
            return;
        }

        // 用文本操作实现折行：移动到折行点，删除行尾，插入换行和 after，恢复光标。
        self.textarea.move_cursor(CursorMove::Head);
        for _ in 0..best_break {
            self.textarea.move_cursor(CursorMove::Forward);
        }
        let after: String = safe_char_slice(&chars, best_break, chars.len())
            .iter()
            .collect();
        self.textarea.delete_line_by_end();
        self.textarea.insert_newline();
        self.textarea.insert_str(&after);

        if col >= best_break {
            let new_col = col - best_break;
            self.textarea.move_cursor(CursorMove::Head);
            for _ in 0..new_col {
                self.textarea.move_cursor(CursorMove::Forward);
            }
        } else {
            self.textarea.move_cursor(CursorMove::Up);
            self.textarea.move_cursor(CursorMove::Head);
            for _ in 0..col {
                self.textarea.move_cursor(CursorMove::Forward);
            }
        }
    }

    /// Handle a backspace
    pub fn backspace(&mut self) {
        self.textarea.delete_char();
        self.hide_suggestions();
        self.reset_history_nav(); // Reset history navigation when deleting
    }

    /// Handle enter key - returns true if should send
    pub fn enter(&mut self, alt: bool) -> bool {
        if alt {
            self.textarea.insert_newline();
            false
        } else if self.show_suggestions {
            self.accept_suggestion();
            false
        } else {
            true
        }
    }

    /// Move cursor left
    pub fn move_left(&mut self) {
        self.textarea.move_cursor(CursorMove::Back);
        self.hide_suggestions();
        self.reset_history_nav();
    }

    /// Move cursor right
    pub fn move_right(&mut self) {
        self.textarea.move_cursor(CursorMove::Forward);
        self.hide_suggestions();
        self.reset_history_nav();
    }

    /// Move cursor up (or select previous suggestion, or browse history)
    pub fn move_up(&mut self) {
        if self.select_previous() {
            return;
        }

        let (row, _) = self.textarea.cursor();
        if row == 0 {
            // 仅当 input 非空或已在历史浏览模式时才触发历史翻看
            if !self.is_empty() || self.history_index.is_some() {
                self.history_up();
            }
        } else {
            self.textarea.move_cursor(CursorMove::Up);
        }
    }

    /// Move cursor down (or select next suggestion, or browse history)
    pub fn move_down(&mut self) {
        if self.select_next() {
            return;
        }

        let (row, _) = self.textarea.cursor();
        let line_count = self.textarea.lines().len();
        if row >= line_count - 1 {
            self.history_down();
        } else {
            self.textarea.move_cursor(CursorMove::Down);
        }
    }

    /// Move cursor to start of line
    pub fn move_home(&mut self) {
        self.textarea.move_cursor(CursorMove::Head);
        self.hide_suggestions();
    }

    /// Move cursor to end of line
    pub fn move_end(&mut self) {
        self.textarea.move_cursor(CursorMove::End);
        self.hide_suggestions();
    }

    /// Delete word
    pub fn delete_word(&mut self) {
        self.textarea.delete_word();
        self.hide_suggestions();
    }

    /// Set text content
    pub fn set_text(&mut self, text: &str) {
        self.textarea.delete_line_by_head();
        for line in text.lines() {
            self.textarea.insert_str(line);
        }
    }
}

fn display_width(chars: &[char]) -> usize {
    chars.iter().map(|&c| char_width(c)).sum()
}

fn find_wrap_break(chars: &[char], max_w: usize) -> usize {
    let mut best_break = 0;
    let mut current_width = 0;
    for (i, ch) in chars.iter().enumerate() {
        let w = char_width(*ch);
        if current_width + w > max_w {
            break;
        }
        if *ch == ' ' {
            best_break = i + 1;
        }
        current_width += w;
    }

    if best_break != 0 {
        return best_break;
    }

    let mut width = 0;
    for (i, ch) in chars.iter().enumerate() {
        width += char_width(*ch);
        if width > max_w {
            return i;
        }
    }
    0
}

fn char_width(ch: char) -> usize {
    if ch == '\t' {
        4
    } else {
        ch.width().unwrap_or(1)
    }
}

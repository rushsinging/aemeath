// Suggestion 在 apply_current_suggestion 中通过 self.input_area.accept_suggestion() 使用

impl super::App {
    /// Copy text to clipboard with status bar feedback.
    pub fn copy_to_clipboard(&mut self, text: &str) {
        match crate::tui::clipboard::copy_text(text) {
            Ok(()) => self.status_bar.set_success("已复制选中内容"),
            Err(err) => {
                log::warn!("复制选中内容失败: {err}");
                self.status_bar.set_warning(&err);
            }
        }
    }

    /// Copy optional selection text to clipboard; no-op if None.
    pub fn copy_selection_to_clipboard(&mut self, text: Option<String>) {
        if let Some(t) = text {
            self.copy_to_clipboard(&t);
        }
    }

    /// Accept the currently highlighted suggestion
    pub fn apply_current_suggestion(&mut self) {
        use crate::tui::completion::extract_completion_token;
        use crate::tui::completion::SuggestionType;

        if let Some(suggestion) = self.input_area.accept_suggestion() {
            let current = self.input_area.get_text();
            let (_row, col) = self.input_area.cursor_position();
            // 将列号（字符计数）转换为字节偏移
            let cursor_offset = current
                .char_indices()
                .nth(col)
                .map(|(i, _)| i)
                .unwrap_or(current.len());

            match suggestion.suggestion_type {
                SuggestionType::Session => {
                    // display_text = "session_id  summary [Nmsg]"
                    // 只取 session_id 部分，替换 /resume 后的参数
                    let id = suggestion
                        .display_text
                        .split_whitespace()
                        .next()
                        .unwrap_or("");
                    if let Some(space_pos) = current.find(' ') {
                        let prefix = current.get(..=space_pos).unwrap_or("");
                        self.input_area.set_text(&format!("{}{}", prefix, id));
                    } else {
                        self.input_area.set_text(&format!("/resume {}", id));
                    }
                }
                _ => {
                    // 用 extract_completion_token 拿到触发器起始位置
                    let replacement = &suggestion.display_text;
                    let new_text = if let Some((_token, start_pos, trigger_type)) =
                        extract_completion_token(&current, cursor_offset)
                    {
                        match trigger_type {
                            // @ 补全：token 已经包含 @，但 display_text 不含 @，
                            // 所以需要保留 @ 前缀，只替换 @ 后面的路径部分
                            crate::tui::completion::TriggerType::AtSymbol => {
                                // start_pos 是 @ 的位置
                                let before = current.get(..start_pos).unwrap_or("");
                                let after_end = find_token_end(&current, cursor_offset);
                                let after = current.get(after_end..).unwrap_or("");
                                format!("{}@{}{}", before, replacement, after)
                            }
                            // / 命令补全：start_pos 是 / 的位置，display_text 包含 /
                            crate::tui::completion::TriggerType::SlashCommand => {
                                let before = current.get(..start_pos).unwrap_or("");
                                let after_end = find_token_end(&current, cursor_offset);
                                let after = current.get(after_end..).unwrap_or("");
                                format!("{}{}{}", before, replacement, after)
                            }
                            // /model 或 /model 子命令补全：起始位置是参数开始处
                            crate::tui::completion::TriggerType::ModelArg
                            | crate::tui::completion::TriggerType::ModelSubCommand => {
                                let before = current.get(..start_pos).unwrap_or("");
                                let after_end = find_token_end(&current, cursor_offset);
                                let after = current.get(after_end..).unwrap_or("");
                                format!("{}{}{}", before, replacement, after)
                            }
                            // /resume 参数补全：起始位置是参数开始处
                            crate::tui::completion::TriggerType::ResumeArg => {
                                let before = current.get(..start_pos).unwrap_or("");
                                let after_end = find_token_end(&current, cursor_offset);
                                let after = current.get(after_end..).unwrap_or("");
                                format!("{}{}{}", before, replacement, after)
                            }
                        }
                    } else {
                        // 没有匹配到触发器，回退到全量替换
                        replacement.clone()
                    };
                    self.input_area.set_text(&new_text);
                    // 将光标移到末尾
                    self.input_area.move_end();
                }
            }
        }
    }
}

/// 找到光标位置处当前 token 的结束字节位置（到空格或行尾）
fn find_token_end(input: &str, cursor_offset: usize) -> usize {
    let remaining = input.get(cursor_offset..).unwrap_or("");
    if let Some(space_pos) = remaining.find(' ') {
        cursor_offset + space_pos
    } else {
        input.len()
    }
}

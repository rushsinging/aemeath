use crate::tui::effect::effect::Effect;

impl super::App {
    /// 描述「复制文本到剪贴板」副作用，返回 CopyToClipboard Effect（不在此处做 IO）。
    /// 实际的剪贴板写入与 status bar 反馈由 effect/executor 执行。
    pub fn copy_to_clipboard(&self, text: &str) -> Effect {
        Effect::CopyToClipboard {
            text: text.to_string(),
        }
    }

    /// 描述「复制可选选区文本」副作用；None 时返回 None（不复制）。
    pub fn copy_selection_to_clipboard(&self, text: Option<String>) -> Option<Effect> {
        text.map(|t| self.copy_to_clipboard(&t))
    }

    /// Accept the currently highlighted suggestion
    pub fn apply_current_suggestion(&mut self) {
        use crate::tui::model::input::completion::extract_completion_token;
        use crate::tui::model::input::completion::SuggestionType;

        if let Some(suggestion) = self.input_area.selected_suggestion().cloned() {
            let current = self.model.input.document.buffer.clone();
            let cursor_offset = self.model.input.document.cursor;

            let new_text = match suggestion.suggestion_type {
                SuggestionType::Session => {
                    let id = suggestion
                        .display_text
                        .split_whitespace()
                        .next()
                        .unwrap_or("");
                    if let Some(space_pos) = current.find(' ') {
                        let prefix = current.get(..=space_pos).unwrap_or("");
                        format!("{}{}", prefix, id)
                    } else {
                        format!("/resume {}", id)
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
                            crate::tui::model::input::completion::TriggerType::AtSymbol => {
                                // start_pos 是 @ 的位置
                                let before = current.get(..start_pos).unwrap_or("");
                                let after_end = find_token_end(&current, cursor_offset);
                                let after = current.get(after_end..).unwrap_or("");
                                format!("{}@{}{}", before, replacement, after)
                            }
                            // / 命令补全：start_pos 是 / 的位置，display_text 包含 /
                            crate::tui::model::input::completion::TriggerType::SlashCommand => {
                                let before = current.get(..start_pos).unwrap_or("");
                                let after_end = find_token_end(&current, cursor_offset);
                                let after = current.get(after_end..).unwrap_or("");
                                format!("{}{}{}", before, replacement, after)
                            }
                            // /model 或 /model 子命令补全：起始位置是参数开始处
                            crate::tui::model::input::completion::TriggerType::ModelArg
                            | crate::tui::model::input::completion::TriggerType::ModelSubCommand => {
                                let before = current.get(..start_pos).unwrap_or("");
                                let after_end = find_token_end(&current, cursor_offset);
                                let after = current.get(after_end..).unwrap_or("");
                                format!("{}{}{}", before, replacement, after)
                            }
                            // /resume 参数补全：起始位置是参数开始处
                            crate::tui::model::input::completion::TriggerType::ResumeArg => {
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
                    new_text
                }
            };
            self.handle_input_intent(
                crate::tui::model::input::intent::InputIntent::AcceptCompletionValue(new_text),
            );
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

#[cfg(test)]
mod tests {
    use crate::tui::app::App;
    use crate::tui::effect::effect::Effect;
    use std::path::PathBuf;

    fn make_app() -> App {
        App::new("s".to_string(), PathBuf::from("/tmp"), "m".to_string())
    }

    #[test]
    fn test_copy_to_clipboard_returns_effect() {
        let app = make_app();
        let effect = app.copy_to_clipboard("hello");
        assert!(matches!(effect, Effect::CopyToClipboard { text } if text == "hello"));
    }

    #[test]
    fn test_copy_selection_to_clipboard_some_returns_effect() {
        let app = make_app();
        let effect = app.copy_selection_to_clipboard(Some("sel".to_string()));
        assert!(matches!(
            effect,
            Some(Effect::CopyToClipboard { text }) if text == "sel"
        ));
    }

    #[test]
    fn test_copy_selection_to_clipboard_none_returns_none() {
        let app = make_app();
        assert!(app.copy_selection_to_clipboard(None).is_none());
    }

    #[test]
    fn test_find_token_end_with_space() {
        assert_eq!(super::find_token_end("ab cd", 0), 2);
    }

    #[test]
    fn test_find_token_end_no_space_returns_len() {
        assert_eq!(super::find_token_end("abcd", 0), 4);
    }

    #[test]
    fn test_find_token_end_cursor_past_space() {
        // cursor 在空格之后，下一个空格不存在 -> 返回总长度。
        assert_eq!(super::find_token_end("ab cd", 3), 5);
    }
}

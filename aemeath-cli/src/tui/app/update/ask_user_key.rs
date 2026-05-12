use super::UpdateResult;
use crate::tui::app::msg::Cmd;
use crate::tui::app::App;
use crossterm::event::{KeyCode, KeyModifiers};

impl App {
    pub(super) fn update_ask_user_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<UpdateResult> {
        // AskUserQuestion 交互模式（有选项列表）
        if let Some(ref state) = self.ask_user_state {
            let options_count = state.options.len();
            let multi_select = state.multi_select;

            match key.code {
                KeyCode::Up if key.modifiers == KeyModifiers::NONE => {
                    if options_count > 0 {
                        let cursor = if state.cursor == 0 {
                            options_count - 1
                        } else {
                            state.cursor - 1
                        };
                        self.ask_user_state.as_mut().unwrap().cursor = cursor;
                        let s = self.ask_user_state.as_ref().unwrap();
                        self.output_area.update_ask_user_options(
                            s.option_line_start,
                            &s.options,
                            s.cursor,
                            s.multi_select,
                            &s.selected,
                        );
                    }
                }
                KeyCode::Down if key.modifiers == KeyModifiers::NONE => {
                    if options_count > 0 {
                        let cursor = (state.cursor + 1) % options_count;
                        self.ask_user_state.as_mut().unwrap().cursor = cursor;
                        let s = self.ask_user_state.as_ref().unwrap();
                        self.output_area.update_ask_user_options(
                            s.option_line_start,
                            &s.options,
                            s.cursor,
                            s.multi_select,
                            &s.selected,
                        );
                    }
                }
                KeyCode::Char(' ') if key.modifiers == KeyModifiers::NONE && multi_select => {
                    let idx = state.cursor;
                    self.ask_user_state.as_mut().unwrap().selected[idx] = !state.selected[idx];
                    let s = self.ask_user_state.as_ref().unwrap();
                    self.output_area.update_ask_user_options(
                        s.option_line_start,
                        &s.options,
                        s.cursor,
                        s.multi_select,
                        &s.selected,
                    );
                }
                KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                    let state = self.ask_user_state.take().unwrap();
                    let answer = if multi_select {
                        // 多选：返回所有选中项的文本，逗号分隔
                        let selected: Vec<&str> = state
                            .selected
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| **s)
                            .map(|(i, _)| state.options[i].as_str())
                            .collect();
                        if selected.is_empty() {
                            // 没选任何项，返回光标所在项
                            state.options[state.cursor].clone()
                        } else {
                            selected.join(", ")
                        }
                    } else if options_count > 0 {
                        // 单选：返回光标所在项
                        state.options[state.cursor].clone()
                    } else {
                        // 无选项：取输入框文本
                        let text = self.input_area.get_text();
                        if text.is_empty() {
                            String::new()
                        } else {
                            text
                        }
                    };
                    if !answer.is_empty() {
                        self.output_area.push_user_message(&answer);
                    }
                    self.input_area.clear();
                    let _ = state.reply_tx.send(answer);
                    self.output_area.set_spinner_phase("Generating...");
                }
                KeyCode::Esc => {
                    let state = self.ask_user_state.take().unwrap();
                    self.input_area.clear();
                    let _ = state.reply_tx.send(String::new());
                    self.output_area.set_spinner_phase("Generating...");
                }
                _ => {
                    // 普通按键传递给 input_area（用于自由输入模式）
                    self.update_ask_user_input_key(key);
                }
            }
            return Some(UpdateResult {
                cmd: Cmd::None,
                pending_slash: None,
            });
        }

        // AskUserQuestion 自由输入模式（无选项列表，等待 reply_tx）
        if self.ask_user_reply_tx.is_some() {
            match key.code {
                KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                    let text = self.input_area.get_text();
                    if !text.is_empty() {
                        if let Some(reply_tx) = self.ask_user_reply_tx.take() {
                            self.output_area.push_user_message(&text);
                            self.input_area.clear();
                            let _ = reply_tx.send(text);
                            self.output_area.set_spinner_phase("Generating...");
                        }
                    }
                    return Some(UpdateResult {
                        cmd: Cmd::None,
                        pending_slash: None,
                    });
                }
                KeyCode::Esc => {
                    if let Some(reply_tx) = self.ask_user_reply_tx.take() {
                        self.input_area.clear();
                        let _ = reply_tx.send(String::new());
                        self.output_area.set_spinner_phase("Generating...");
                    }
                    return Some(UpdateResult {
                        cmd: Cmd::None,
                        pending_slash: None,
                    });
                }
                // 其他按键传递给 input_area
                _ => {
                    self.update_ask_user_input_key(key);
                    return Some(UpdateResult {
                        cmd: Cmd::None,
                        pending_slash: None,
                    });
                }
            }
        }

        None
    }

    fn update_ask_user_input_key(&mut self, key: crossterm::event::KeyEvent) {
        // Shift+Enter / Alt+Enter = 换行
        if key.code == KeyCode::Enter
            && key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
        {
            self.input_area.enter(true);
            return;
        }
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                let ch = if key.modifiers.contains(KeyModifiers::SHIFT) {
                    c.to_ascii_uppercase()
                } else {
                    c
                };
                self.input_area.input(ch);
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => {
                self.input_area.backspace();
            }
            (KeyModifiers::NONE, KeyCode::Left) => self.input_area.move_left(),
            (KeyModifiers::NONE, KeyCode::Right) => self.input_area.move_right(),
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => self.input_area.move_home(),
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => self.input_area.move_end(),
            (KeyModifiers::CONTROL, KeyCode::Char('w')) => self.input_area.delete_word(),
            _ => {}
        }
    }
}

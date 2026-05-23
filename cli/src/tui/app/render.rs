use crate::tui::output_area::{LineStyle, OutputLine};
use aemeath_core::message::{Message, Role};

impl super::App {
    /// Render a saved history message into the output area (used during session resume).
    ///
    /// For assistant messages containing ToolUse blocks, this pairs each ToolUse with the
    /// corresponding ToolResult from the following user message (by matching `tool_use_id`),
    /// and renders them using the same ToolDisplay format as the live path
    /// (`push_tool_call` + `push_tool_result_with_diff`).
    pub fn render_history_message(&mut self, msg: &Message, subsequent_msg: Option<&Message>) {
        match msg.role {
            Role::User => {
                // User 消息可能包含 ToolResult（API 规范中 tool result 放在 user 消息里）
                // 这些 ToolResult 由前一条 Assistant 消息中的 ToolUse 配对渲染，这里只处理用户文本。
                let mut has_text = false;

                for block in &msg.content {
                    if let aemeath_core::message::ContentBlock::Text { text } = block {
                        if !text.trim().is_empty() {
                            has_text = true;
                        }
                    }
                }

                if has_text {
                    let user_text = msg
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            aemeath_core::message::ContentBlock::Text { text } => {
                                Some(text.as_str())
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    self.output_area.push_line(OutputLine {
                        content: format!("> {}", user_text),
                        style: LineStyle::User,
                        ..Default::default()
                    });
                }
            }
            Role::Assistant => {
                // Collect ToolResult blocks from the following user message for pairing.
                let tool_results: std::collections::HashMap<&str, (&serde_json::Value, bool)> =
                    if let Some(user_msg) = subsequent_msg {
                        user_msg
                            .content
                            .iter()
                            .filter_map(|block| match block {
                                aemeath_core::message::ContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                    is_error,
                                    ..
                                } => Some((tool_use_id.as_str(), (content, *is_error))),
                                _ => None,
                            })
                            .collect()
                    } else {
                        std::collections::HashMap::new()
                    };

                for (_, block) in msg.content.iter().enumerate() {
                    match block {
                        aemeath_core::message::ContentBlock::Text { text } => {
                            for text_line in text.lines() {
                                self.output_area.push_line(OutputLine {
                                    content: text_line.to_string(),
                                    style: LineStyle::Assistant,
                                    ..Default::default()
                                });
                            }
                        }
                        aemeath_core::message::ContentBlock::ToolUse {
                            id, name, input, ..
                        } => {
                            let input_json = input.to_string();
                            // Use the same tool_id as live path so styles match.
                            let tool_id = format!("resume:{id}");
                            let name_str = name.as_str();

                            // Render tool call header + details (live-path style)
                            self.output_area
                                .push_tool_call(&tool_id, name_str, &input_json);

                            // Pair with the corresponding ToolResult if available
                            if let Some((content, is_error)) = tool_results.get(id.as_str()) {
                                let result_str = match content {
                                    serde_json::Value::String(s) => s.clone(),
                                    serde_json::Value::Array(arr) => arr
                                        .iter()
                                        .filter_map(|v| v.get("text").and_then(|t| t.as_str()))
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                    _ => content.to_string(),
                                };
                                self.output_area.push_tool_result_with_diff(
                                    &tool_id,
                                    name_str,
                                    &result_str,
                                    *is_error,
                                    &"",
                                );
                            } else {
                                // No paired result — mark as completed but without result content.
                                // Mark the header as done.
                                for line in self.output_area.lines.iter_mut() {
                                    if line.tool_id.as_deref() == Some(&tool_id) {
                                        line.content = line.content.replacen('●', "✓", 1);
                                        line.style = LineStyle::ToolCallSuccess;
                                        break;
                                    }
                                }
                            }
                        }
                        aemeath_core::message::ContentBlock::ToolResult { .. } => {
                            // ToolResult blocks in assistant messages are rendered by
                            // push_tool_result_with_diff above (matched by id).
                        }
                        _ => {}
                    }
                }
                self.output_area.push_line(OutputLine {
                    content: String::new(),
                    style: LineStyle::System,
                    ..Default::default()
                });
            }
        }
    }
}

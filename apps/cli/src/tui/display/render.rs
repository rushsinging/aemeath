use crate::tui::output_area::{LineStyle, OutputLine};

impl crate::tui::core::App {
    /// Render a saved history message into the output area (used during session resume).
    ///
    /// For assistant messages containing ToolUse blocks, this pairs each ToolUse with the
    /// corresponding ToolResult from the following user message (by matching `tool_use_id`),
    /// and renders them using the same ToolDisplay format as the live path
    /// (`push_tool_call` + `push_tool_result_with_diff`).
    pub fn render_history_message(
        &mut self,
        msg: &sdk::ChatMessage,
        subsequent_msg: Option<&sdk::ChatMessage>,
    ) {
        match msg.role.as_str() {
            "user" => {
                // User 消息可能包含 ToolResult（API 规范中 tool result 放在 user 消息里）
                // 这些 ToolResult 由前一条 Assistant 消息中的 ToolUse 配对渲染，这里只处理用户文本。
                let mut has_text = false;

                for block in msg.content.as_array().into_iter().flatten() {
                    if block.get("type").and_then(|value| value.as_str()) == Some("text")
                        && block
                            .get("text")
                            .and_then(|text| text.as_str())
                            .is_some_and(|text| !text.trim().is_empty())
                    {
                        has_text = true;
                    }
                }

                if has_text {
                    let user_text = msg.text_content();
                    self.output_area.push_line(OutputLine {
                        content: format!("> {}", user_text),
                        style: LineStyle::User,
                        ..Default::default()
                    });
                }
            }
            "assistant" => {
                // Collect ToolResult blocks from the following user message for pairing.
                let tool_results: std::collections::HashMap<&str, (&serde_json::Value, bool)> =
                    if let Some(user_msg) = subsequent_msg {
                        user_msg
                            .content
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|block| {
                                if block.get("type").and_then(|value| value.as_str())
                                    != Some("tool_result")
                                {
                                    return None;
                                }
                                let tool_use_id = block.get("tool_use_id")?.as_str()?;
                                let content = block.get("content")?;
                                let is_error = block
                                    .get("is_error")
                                    .and_then(|value| value.as_bool())
                                    .unwrap_or(false);
                                Some((tool_use_id, (content, is_error)))
                            })
                            .collect()
                    } else {
                        std::collections::HashMap::new()
                    };

                for block in msg.content.as_array().into_iter().flatten() {
                    match block.get("type").and_then(|value| value.as_str()) {
                        Some("text") => {
                            let text = block
                                .get("text")
                                .and_then(|value| value.as_str())
                                .unwrap_or("");
                            for text_line in text.lines() {
                                self.output_area.push_line(OutputLine {
                                    content: text_line.to_string(),
                                    style: LineStyle::Assistant,
                                    ..Default::default()
                                });
                            }
                        }
                        Some("tool_use") => {
                            let Some(id) = block.get("id").and_then(|value| value.as_str()) else {
                                continue;
                            };
                            let name = block
                                .get("name")
                                .and_then(|value| value.as_str())
                                .unwrap_or("?");
                            let input = block
                                .get("input")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null);
                            let input_json = input.to_string();
                            // Use the same tool_id as live path so styles match.
                            let tool_id = format!("resume:{id}");
                            let name_str = name;

                            // Render tool call header + details (live-path style)
                            self.output_area
                                .push_tool_call(&tool_id, name_str, &input_json);

                            // Pair with the corresponding ToolResult if available
                            if let Some((content, is_error)) = tool_results.get(id) {
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
                                    "",
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
                        Some("tool_result") => {
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
            _ => {}
        }
    }
}

use crate::tui::model::conversation::intent::ConversationIntent;

impl crate::tui::app::App {
    /// Load a saved history message into the TUI model (used during session resume).
    ///
    /// Resume keeps the visual format that users already see, but the source of truth is now
    /// `ConversationModel -> OutputViewAssembler -> OutputArea` instead of direct OutputArea writes.
    pub fn render_history_message(
        &mut self,
        msg: &sdk::ChatMessage,
        subsequent_msg: Option<&sdk::ChatMessage>,
    ) {
        match msg.role.as_str() {
            "user" => self.load_history_user_message(msg),
            "assistant" => self.load_history_assistant_message(msg, subsequent_msg),
            _ => {}
        }
        self.mark_output_dirty();
    }

    fn load_history_user_message(&mut self, msg: &sdk::ChatMessage) {
        let user_text = user_visible_text(msg);
        if user_text.is_empty() {
            return;
        }
        self.model
            .conversation
            .apply(ConversationIntent::StartChat {
                submission: user_text,
            });
    }

    fn load_history_assistant_message(
        &mut self,
        msg: &sdk::ChatMessage,
        subsequent_msg: Option<&sdk::ChatMessage>,
    ) {
        let tool_results = collect_following_tool_results(subsequent_msg);
        for (index, block) in msg.content.as_array().into_iter().flatten().enumerate() {
            match block.get("type").and_then(|value| value.as_str()) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(|value| value.as_str()) {
                        self.model
                            .conversation
                            .apply(ConversationIntent::ObserveAssistantText {
                                text: text.to_string(),
                            });
                        self.model
                            .conversation
                            .apply(ConversationIntent::CompleteTextBlock);
                    }
                }
                Some("thinking") => {
                    if let Some(text) = block
                        .get("thinking")
                        .or_else(|| block.get("text"))
                        .and_then(|value| value.as_str())
                    {
                        self.model
                            .conversation
                            .apply(ConversationIntent::ObserveThinkingText {
                                text: text.to_string(),
                            });
                        self.model
                            .conversation
                            .apply(ConversationIntent::CompleteTextBlock);
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
                    self.model
                        .conversation
                        .apply(ConversationIntent::ObserveToolCallStart {
                            name: name.to_string(),
                            index,
                        });
                    self.model
                        .conversation
                        .apply(ConversationIntent::ObserveToolCall {
                            id: id.to_string(),
                            name: name.to_string(),
                            index,
                            summary: input_json,
                        });
                    if let Some((content, is_error)) = tool_results.get(id) {
                        self.model
                            .conversation
                            .apply(ConversationIntent::ObserveToolResult {
                                id: id.to_string(),
                                tool_name: name.to_string(),
                                output: tool_result_content_to_string(content),
                                is_error: *is_error,
                                image_count: tool_result_image_count(content),
                            });
                    }
                }
                Some("tool_result") => {}
                _ => {}
            }
        }
    }
}

fn user_visible_text(msg: &sdk::ChatMessage) -> String {
    msg.content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(|value| value.as_str()) == Some("text"))
        .filter_map(|block| block.get("text").and_then(|text| text.as_str()))
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("")
}

fn collect_following_tool_results(
    subsequent_msg: Option<&sdk::ChatMessage>,
) -> std::collections::HashMap<&str, (&serde_json::Value, bool)> {
    let Some(user_msg) = subsequent_msg else {
        return std::collections::HashMap::new();
    };
    user_msg
        .content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| {
            if block.get("type").and_then(|value| value.as_str()) != Some("tool_result") {
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
}

fn tool_result_content_to_string(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|value| value.get("text").and_then(|text| text.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => content.to_string(),
    }
}

fn tool_result_image_count(content: &serde_json::Value) -> usize {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|value| value.get("type").and_then(|kind| kind.as_str()) == Some("image"))
        .count()
}

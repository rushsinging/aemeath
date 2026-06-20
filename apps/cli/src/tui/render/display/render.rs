use crate::tui::model::conversation::ids::{ChatId, ChatTurnId, ToolCallId};
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::conversation::tool_call::ToolCallStatus;

const HISTORY_RESTORE_ERROR: &str = "无法恢复一条历史消息：消息格式不符合当前会话 schema，已跳过。";

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
        match HistoryDisplayMessage::parse(msg) {
            Ok(HistoryDisplayMessage::User { text }) => self.load_history_user_message(text),
            Ok(HistoryDisplayMessage::ToolResults) => {}
            Ok(HistoryDisplayMessage::Assistant { blocks }) => {
                self.load_history_assistant_message(blocks, subsequent_msg)
            }
            Err(error) => self.report_history_restore_error(error),
        }
        self.mark_output_dirty();
    }

    fn load_history_user_message(&mut self, user_text: String) {
        self.model
            .conversation
            .apply(ConversationIntent::StartChat {
                submission: user_text,
            });
    }

    fn load_history_assistant_message(
        &mut self,
        blocks: Vec<HistoryAssistantBlock>,
        subsequent_msg: Option<&sdk::ChatMessage>,
    ) {
        let chat_id = self
            .model
            .conversation
            .active_chat_id
            .clone()
            .unwrap_or_else(|| ChatId::from_legacy_or_new("history-chat"));
        let turn_id = ChatTurnId::from_legacy_or_new("turn-1");
        self.model
            .conversation
            .ensure_runtime_turn(chat_id.clone(), turn_id.clone());
        let tool_results = collect_following_tool_results(subsequent_msg);
        for (index, block) in blocks.into_iter().enumerate() {
            match block {
                HistoryAssistantBlock::Text(text) => {
                    self.model
                        .conversation
                        .apply(ConversationIntent::ObserveAssistantText {
                            chat_id: chat_id.clone(),
                            turn_id: turn_id.clone(),
                            text,
                        });
                    self.model
                        .conversation
                        .apply(ConversationIntent::CompleteBlock {
                            chat_id: chat_id.clone(),
                            turn_id: turn_id.clone(),
                        });
                }
                HistoryAssistantBlock::Thinking(text) => {
                    self.model
                        .conversation
                        .apply(ConversationIntent::ObserveThinkingText {
                            chat_id: chat_id.clone(),
                            turn_id: turn_id.clone(),
                            text,
                        });
                    self.model
                        .conversation
                        .apply(ConversationIntent::CompleteBlock {
                            chat_id: chat_id.clone(),
                            turn_id: turn_id.clone(),
                        });
                }
                HistoryAssistantBlock::ToolUse { id, name, input } => {
                    let input_json = input.to_string();
                    let tool_call_id = ToolCallId::from_legacy_or_new(&id);
                    self.model
                        .conversation
                        .apply(ConversationIntent::ObserveToolCallStart {
                            chat_id: chat_id.clone(),
                            turn_id: turn_id.clone(),
                            id: tool_call_id.clone(),
                            provider_id: None,
                            name: name.clone(),
                            index,
                        });
                    self.model
                        .conversation
                        .apply(ConversationIntent::ObserveToolCallUpdate {
                            chat_id: chat_id.clone(),
                            turn_id: turn_id.clone(),
                            id: tool_call_id.clone(),
                            provider_id: Some(id.clone()),
                            name: name.clone(),
                            index,
                            arguments: Some(input_json.clone()),
                            status: ToolCallStatus::Ready,
                        });
                    if let Some(result) = tool_results.get(id.as_str()) {
                        self.model
                            .conversation
                            .apply(ConversationIntent::ObserveToolResult {
                                chat_id: chat_id.clone(),
                                turn_id: turn_id.clone(),
                                id: tool_call_id.clone(),
                                provider_id: id.clone(),
                                tool_name: name,
                                output: tool_result_content_to_string(result.content),
                                content: normalize_tool_result_content(result.content),
                                is_error: result.is_error,
                                image_count: tool_result_image_count(result.content),
                            });
                    }
                }
            }
        }
    }

    fn report_history_restore_error(&mut self, error: HistoryDisplayParseError) {
        crate::tui::log_warn!("skip invalid history message during resume: {error}");
        self.model
            .conversation
            .apply(ConversationIntent::AppendError {
                text: HISTORY_RESTORE_ERROR.to_string(),
            });
    }
}

#[derive(Debug, Eq, PartialEq)]
enum HistoryDisplayMessage {
    User { text: String },
    ToolResults,
    Assistant { blocks: Vec<HistoryAssistantBlock> },
}

#[derive(Debug, Eq, PartialEq)]
enum HistoryAssistantBlock {
    Text(String),
    Thinking(String),
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Eq, PartialEq)]
enum HistoryDisplayParseError {
    UnsupportedRole(String),
    ContentNotArray,
    BlockNotObject,
    MissingBlockType,
    UnsupportedUserBlock(String),
    UnsupportedAssistantBlock(String),
    MissingText,
    EmptyUserText,
    MissingToolUseId,
    MissingToolUseName,
    MissingToolUseInput,
    MissingToolResultId,
    MissingToolResultContent,
    EmptyAssistantMessage,
}

impl std::fmt::Display for HistoryDisplayParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl HistoryDisplayMessage {
    fn parse(msg: &sdk::ChatMessage) -> Result<Self, HistoryDisplayParseError> {
        let blocks = msg.content.as_slice();
        match msg.role.as_str() {
            "user" => parse_history_user(blocks),
            "assistant" => parse_history_assistant(blocks),
            role => Err(HistoryDisplayParseError::UnsupportedRole(role.to_string())),
        }
    }
}

fn parse_history_user(
    blocks: &[sdk::ContentBlock],
) -> Result<HistoryDisplayMessage, HistoryDisplayParseError> {
    let parsed_blocks = parse_history_user_blocks(blocks)?;
    let mut text = String::new();
    let mut has_tool_result = false;
    for block in parsed_blocks {
        match block {
            HistoryUserBlock::Text(block_text) => text.push_str(block_text),
            HistoryUserBlock::ToolResult { .. } => has_tool_result = true,
        }
    }
    if text.trim().is_empty() {
        return if has_tool_result {
            Ok(HistoryDisplayMessage::ToolResults)
        } else {
            Err(HistoryDisplayParseError::EmptyUserText)
        };
    }
    Ok(HistoryDisplayMessage::User { text })
}

fn parse_history_assistant(
    blocks: &[sdk::ContentBlock],
) -> Result<HistoryDisplayMessage, HistoryDisplayParseError> {
    let mut parsed = Vec::new();
    for block in blocks {
        match block {
            sdk::ContentBlock::Text { text } => {
                parsed.push(HistoryAssistantBlock::Text(text.clone()));
            }
            sdk::ContentBlock::Thinking { thinking } => {
                parsed.push(HistoryAssistantBlock::Thinking(thinking.clone()));
            }
            sdk::ContentBlock::ToolUse { id, name, input } => {
                parsed.push(HistoryAssistantBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                });
            }
            sdk::ContentBlock::ToolResult { .. } => {
                return Err(HistoryDisplayParseError::UnsupportedAssistantBlock(
                    "tool_result".to_string(),
                ))
            }
            sdk::ContentBlock::Image { .. } => {
                return Err(HistoryDisplayParseError::UnsupportedAssistantBlock(
                    "image".to_string(),
                ))
            }
        }
    }
    if parsed.is_empty() {
        return Err(HistoryDisplayParseError::EmptyAssistantMessage);
    }
    Ok(HistoryDisplayMessage::Assistant { blocks: parsed })
}

#[derive(Clone, Copy)]
struct HistoryToolResult<'a> {
    content: &'a serde_json::Value,
    is_error: bool,
}

#[derive(Debug, Eq, PartialEq)]
enum HistoryUserBlock<'a> {
    Text(&'a str),
    ToolResult {
        tool_use_id: &'a str,
        content: &'a serde_json::Value,
        is_error: bool,
    },
}

fn parse_history_user_blocks(
    blocks: &[sdk::ContentBlock],
) -> Result<Vec<HistoryUserBlock<'_>>, HistoryDisplayParseError> {
    blocks
        .iter()
        .map(|block| match block {
            sdk::ContentBlock::Text { text } => Ok(HistoryUserBlock::Text(text.as_str())),
            sdk::ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
                ..
            } => Ok(HistoryUserBlock::ToolResult {
                tool_use_id: tool_use_id.as_str(),
                content,
                is_error: *is_error,
            }),
            sdk::ContentBlock::Thinking { .. } => Err(
                HistoryDisplayParseError::UnsupportedUserBlock("thinking".to_string()),
            ),
            sdk::ContentBlock::ToolUse { .. } => Err(
                HistoryDisplayParseError::UnsupportedUserBlock("tool_use".to_string()),
            ),
            sdk::ContentBlock::Image { .. } => Err(HistoryDisplayParseError::UnsupportedUserBlock(
                "image".to_string(),
            )),
        })
        .collect()
}

fn collect_following_tool_results(
    subsequent_msg: Option<&sdk::ChatMessage>,
) -> std::collections::HashMap<&str, HistoryToolResult<'_>> {
    let Some(user_msg) = subsequent_msg else {
        return std::collections::HashMap::new();
    };
    let Ok(parsed_blocks) = parse_history_user_blocks(user_msg.content.as_slice()) else {
        return std::collections::HashMap::new();
    };
    parsed_blocks
        .into_iter()
        .filter_map(|block| match block {
            HistoryUserBlock::Text(_) => None,
            HistoryUserBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => Some((tool_use_id, HistoryToolResult { content, is_error })),
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

fn normalize_tool_result_content(content: &serde_json::Value) -> serde_json::Value {
    match content {
        serde_json::Value::String(text) => serde_json::json!({ "text": text }),
        serde_json::Value::Array(arr) => {
            let text = arr
                .iter()
                .filter_map(|value| value.get("text").and_then(|text| text.as_str()))
                .collect::<Vec<_>>()
                .join("\n");
            serde_json::json!({ "text": text })
        }
        value => value.clone(),
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

#[cfg(test)]
mod tests {
    use crate::tui::app::App;
    use crate::tui::model::conversation::block::ConversationBlock;
    use crate::tui::model::conversation::tool_call::ToolCallStatus;

    fn app() -> App {
        App::new(
            "test-session".to_string(),
            std::path::PathBuf::from("."),
            "test-model".to_string(),
        )
    }

    fn message(role: &str, content: Vec<sdk::ContentBlock>) -> sdk::ChatMessage {
        sdk::ChatMessage {
            role: role.to_string(),
            content,
            metadata: None,
        }
    }

    fn user_text(text: &str) -> sdk::ChatMessage {
        message(
            "user",
            vec![sdk::ContentBlock::Text {
                text: text.to_string(),
            }],
        )
    }

    #[test]
    fn test_render_history_message_renders_schema_user_text() {
        let mut app = app();
        let msg = user_text("hello");

        app.render_history_message(&msg, None);

        assert!(app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::UserMessage { text, .. } if text == "hello")
        }));
    }

    #[test]
    fn test_render_history_message_reports_empty_user_text() {
        let mut app = app();
        let msg = user_text("   ");

        app.render_history_message(&msg, None);

        assert!(!app
            .model
            .conversation
            .blocks
            .iter()
            .any(|block| { matches!(block, ConversationBlock::UserMessage { .. }) }));
        assert!(app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::Error { text, .. } if text.contains("无法恢复一条历史消息"))
        }));
    }

    #[test]
    fn test_render_history_message_renders_assistant_blocks() {
        let mut app = app();
        app.render_history_message(&user_text("hello"), None);
        let msg = message(
            "assistant",
            vec![
                sdk::ContentBlock::Thinking {
                    thinking: "plan".to_string(),
                },
                sdk::ContentBlock::Text {
                    text: "answer".to_string(),
                },
            ],
        );

        app.render_history_message(&msg, None);

        assert!(app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::Thinking { text, .. } if text == "plan")
        }));
        assert!(app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::AssistantText { text, .. } if text == "answer")
        }));
    }

    #[test]
    fn test_render_history_message_links_following_tool_result() {
        let mut app = app();
        app.render_history_message(&user_text("hello"), None);
        let assistant = message(
            "assistant",
            vec![sdk::ContentBlock::ToolUse {
                id: "tool-1".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({ "file_path": "a.rs" }),
            }],
        );
        let tool_result = message(
            "user",
            vec![sdk::ContentBlock::ToolResult {
                tool_use_id: "tool-1".to_string(),
                content: serde_json::json!("done"),
                is_error: false,
                text: None,
            }],
        );

        app.render_history_message(&assistant, Some(&tool_result));

        let expected_tool_id = crate::tui::model::conversation::ids::ToolCallId::new("tool-1");
        let tool_call = app
            .model
            .conversation
            .chats
            .iter()
            .flat_map(|chat| &chat.turns)
            .flat_map(|turn| &turn.tool_calls)
            .find(|call| call.id.as_ref() == Some(&expected_tool_id))
            .expect("tool call should be restored");
        assert_eq!(tool_call.status, ToolCallStatus::Success);
        assert_eq!(tool_call.result.as_deref(), Some("done"));
    }

    #[test]
    fn test_render_history_message_reports_empty_assistant_message() {
        let mut app = app();
        app.render_history_message(&user_text("hello"), None);
        let msg = message("assistant", vec![]);

        app.render_history_message(&msg, None);

        assert!(app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::Error { text, .. } if text.contains("无法恢复一条历史消息"))
        }));
    }

    #[test]
    fn test_render_history_message_reports_unknown_role() {
        let mut app = app();
        let msg = message(
            "system",
            vec![sdk::ContentBlock::Text {
                text: "notice".to_string(),
            }],
        );

        app.render_history_message(&msg, None);

        assert!(app.model.conversation.blocks.iter().any(|block| {
            matches!(block, ConversationBlock::Error { text, .. } if text.contains("无法恢复一条历史消息"))
        }));
    }
}

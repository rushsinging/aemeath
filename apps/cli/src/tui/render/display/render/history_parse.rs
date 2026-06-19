#[derive(Debug, Eq, PartialEq)]
pub(super) enum HistoryDisplayMessage {
    User { text: String },
    ToolResults,
    Assistant { blocks: Vec<HistoryAssistantBlock> },
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum HistoryAssistantBlock {
    Text(String),
    Thinking(String),
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum HistoryDisplayParseError {
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
    pub(super) fn parse(msg: &sdk::ChatMessage) -> Result<Self, HistoryDisplayParseError> {
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
            sdk::ContentBlock::Image { .. } => Err(
                HistoryDisplayParseError::UnsupportedUserBlock("image".to_string()),
            ),
        })
        .collect()
}

pub(super) fn collect_following_tool_results(
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

pub(super) fn tool_result_content_to_string(content: &serde_json::Value) -> String {
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

pub(super) fn normalize_tool_result_content(content: &serde_json::Value) -> serde_json::Value {
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

pub(super) fn tool_result_image_count(content: &serde_json::Value) -> usize {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|value| value.get("type").and_then(|kind| kind.as_str()) == Some("image"))
        .count()
}

use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiContentBlock};

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum HistoryDisplayMessage {
    User {
        text: String,
    },
    HookNotice {
        title: String,
        text: String,
        kind: crate::tui::adapter::runtime_view::TuiHookNoticeKind,
    },
    ToolResults,
    Assistant {
        blocks: Vec<HistoryAssistantBlock>,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum HistoryAssistantBlock {
    Text(String),
    Thinking(String),
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum HistoryDisplayParseError {
    UnsupportedRole(String),
    UnsupportedUserBlock(String),
    UnsupportedAssistantBlock(String),
    EmptyUserText,
    EmptyAssistantMessage,
    NonUserVisibleMessage,
}

impl std::fmt::Display for HistoryDisplayParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl HistoryDisplayMessage {
    pub(crate) fn parse(msg: &TuiChatMessage) -> Result<Self, HistoryDisplayParseError> {
        if msg.role == "user" {
            match msg.source {
                crate::tui::adapter::runtime_view::TuiMessageSource::SkillRequest => {
                    return msg
                        .skill_request
                        .as_ref()
                        .map(|payload| Self::User {
                            text: payload.raw_input.clone(),
                        })
                        .ok_or(HistoryDisplayParseError::NonUserVisibleMessage);
                }
                crate::tui::adapter::runtime_view::TuiMessageSource::Hook => {
                    let notice = msg
                        .hook_notice
                        .as_ref()
                        .ok_or(HistoryDisplayParseError::NonUserVisibleMessage)?;
                    return Ok(Self::HookNotice {
                        title: notice.title(),
                        text: notice.display_text(),
                        kind: notice.kind.clone(),
                    });
                }
                crate::tui::adapter::runtime_view::TuiMessageSource::SystemGenerated => {
                    return Err(HistoryDisplayParseError::NonUserVisibleMessage);
                }
                crate::tui::adapter::runtime_view::TuiMessageSource::User => {}
            }
        }
        let blocks = msg.content.as_slice();
        match msg.role.as_str() {
            "user" => parse_history_user(blocks),
            "assistant" => parse_history_assistant(blocks),
            role => Err(HistoryDisplayParseError::UnsupportedRole(role.to_string())),
        }
    }
}

fn parse_history_user(
    blocks: &[TuiContentBlock],
) -> Result<HistoryDisplayMessage, HistoryDisplayParseError> {
    let parsed_blocks = parse_history_user_blocks(blocks)?;
    let mut text = String::new();
    let mut has_tool_result = false;
    for block in parsed_blocks {
        match block {
            HistoryUserBlock::Text(block_text) => text.push_str(block_text),
            // #fix-tui-image-input-output：image 占位符拼入 text
            HistoryUserBlock::Image(placeholder) => text.push_str(&placeholder),
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
    blocks: &[TuiContentBlock],
) -> Result<HistoryDisplayMessage, HistoryDisplayParseError> {
    let mut parsed = Vec::new();
    for block in blocks {
        match block {
            TuiContentBlock::Text { text } => {
                parsed.push(HistoryAssistantBlock::Text(text.clone()));
            }
            TuiContentBlock::Thinking { thinking, .. } => {
                parsed.push(HistoryAssistantBlock::Thinking(thinking.clone()));
            }
            TuiContentBlock::ToolUse { id, name, input } => {
                parsed.push(HistoryAssistantBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                });
            }
            TuiContentBlock::ToolResult { .. } => {
                return Err(HistoryDisplayParseError::UnsupportedAssistantBlock(
                    "tool_result".to_string(),
                ))
            }
            TuiContentBlock::Image { .. } => {
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
pub(crate) struct HistoryToolResult<'a> {
    pub content: &'a serde_json::Value,
    pub text: Option<&'a str>,
    pub is_error: bool,
}

pub(crate) fn restored_ask_answer(result: HistoryToolResult<'_>) -> Option<String> {
    if result.is_error {
        return None;
    }
    result
        .content
        .get("answer")
        .and_then(serde_json::Value::as_str)
        .filter(|answer| !answer.trim().is_empty())
        .or(result.text.filter(|text| !text.trim().is_empty()))
        .or_else(|| {
            result
                .content
                .as_str()
                .filter(|content| !content.trim().is_empty())
        })
        .map(ToOwned::to_owned)
}

#[derive(Debug, Eq, PartialEq)]
enum HistoryUserBlock<'a> {
    Text(&'a str),
    /// #fix-tui-image-input-output：image block 渲染时还原为占位符
    /// `[Image #N]`（由 SDK ContentBlock::Image.placeholder 携带）。
    /// 拼接时直接推入 `text`，让 resume 后的用户消息文本含占位符。
    /// String owned（不用 `&'a str`）以承载 `placeholder.unwrap_or_else` 的临时值。
    Image(String),
    ToolResult {
        tool_use_id: &'a str,
        content: &'a serde_json::Value,
        text: Option<&'a str>,
        is_error: bool,
    },
}

fn parse_history_user_blocks(
    blocks: &[TuiContentBlock],
) -> Result<Vec<HistoryUserBlock<'_>>, HistoryDisplayParseError> {
    blocks
        .iter()
        .map(|block| match block {
            TuiContentBlock::Text { text } => Ok(HistoryUserBlock::Text(text.as_str())),
            TuiContentBlock::ToolResult {
                tool_use_id,
                content,
                text,
                is_error,
                ..
            } => Ok(HistoryUserBlock::ToolResult {
                tool_use_id: tool_use_id.as_str(),
                content,
                text: text.as_deref(),
                is_error: *is_error,
            }),
            TuiContentBlock::Thinking { .. } => Err(
                HistoryDisplayParseError::UnsupportedUserBlock("thinking".to_string()),
            ),
            TuiContentBlock::ToolUse { .. } => Err(HistoryDisplayParseError::UnsupportedUserBlock(
                "tool_use".to_string(),
            )),
            TuiContentBlock::Image { placeholder, .. } => {
                // #fix-tui-image-input-output：image block 渲染为占位符（[Image #N]），
                // 保留 round-trip 时原占位符；如果 placeholder 为 None（旧 history），
                // 用 `[Image]` 作为兜底。`placeholder` 是 `&Option<String>`，
                // `clone()` 避免移动后无法在其它分支复用。
                Ok(HistoryUserBlock::Image(
                    placeholder.clone().unwrap_or_else(|| "[Image]".to_string()),
                ))
            }
        })
        .collect()
}

pub(crate) fn collect_following_tool_results(
    subsequent_msg: Option<&TuiChatMessage>,
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
            // #fix-tui-image-input-output：image 块不带 tool_use_id，跳过
            HistoryUserBlock::Image(_) => None,
            HistoryUserBlock::ToolResult {
                tool_use_id,
                content,
                text,
                is_error,
            } => Some((
                tool_use_id,
                HistoryToolResult {
                    content,
                    text,
                    is_error,
                },
            )),
        })
        .collect()
}

pub(crate) fn tool_result_display_text(result: HistoryToolResult<'_>) -> String {
    result
        .text
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| tool_result_content_to_string(result.content))
}

pub(crate) fn tool_result_content_to_string(content: &serde_json::Value) -> String {
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

pub(crate) fn normalize_tool_result_content(content: &serde_json::Value) -> serde_json::Value {
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

pub(crate) fn tool_result_image_count(content: &serde_json::Value) -> usize {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|value| value.get("type").and_then(|kind| kind.as_str()) == Some("image"))
        .count()
}

#[cfg(test)]
#[path = "history_parse_tests.rs"]
mod tests;

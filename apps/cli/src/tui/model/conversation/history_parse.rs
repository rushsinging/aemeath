use crate::tui::adapter::runtime_view::{TuiChatMessage, TuiContentBlock, TuiMessageSource};

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum HistoryDisplayMessage {
    User { text: String },
    StopHookNotice { body: String },
    ToolResults,
    Assistant { blocks: Vec<HistoryAssistantBlock> },
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
    pub(crate) fn parse(msg: &TuiChatMessage) -> Result<Self, HistoryDisplayParseError> {
        let body = msg.text_content();
        if msg.source == TuiMessageSource::StopHook
            || (msg.source == TuiMessageSource::SystemGenerated
                && is_legacy_stop_hook_feedback(&body))
        {
            if body.trim().is_empty() {
                return Err(HistoryDisplayParseError::EmptyUserText);
            }
            return Ok(HistoryDisplayMessage::StopHookNotice { body });
        }
        let blocks = msg.content.as_slice();
        match msg.role.as_str() {
            "user" => parse_history_user(blocks),
            "assistant" => parse_history_assistant(blocks),
            role => Err(HistoryDisplayParseError::UnsupportedRole(role.to_string())),
        }
    }
}

fn is_legacy_stop_hook_feedback(text: &str) -> bool {
    let body =
        crate::tui::model::conversation::system_reminder::strip_system_reminder_envelope(text);
    body.contains("Stop hook prevented stopping") || body.contains("Stop hook 阻止了停止")
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
mod tests {
    use super::*;

    fn msg(role: &str, content: Vec<TuiContentBlock>) -> TuiChatMessage {
        TuiChatMessage {
            role: role.to_string(),
            content,
            source: TuiMessageSource::User,
            stop_hook: None,
            input_id: None,
        }
    }

    fn text_block(s: &str) -> TuiContentBlock {
        TuiContentBlock::Text {
            text: s.to_string(),
        }
    }

    fn thinking_block(s: &str) -> TuiContentBlock {
        TuiContentBlock::Thinking {
            thinking: s.to_string(),
            signature: None,
        }
    }

    fn tool_use_block(id: &str, name: &str) -> TuiContentBlock {
        TuiContentBlock::ToolUse {
            id: id.to_string(),
            name: name.to_string(),
            input: serde_json::json!({}),
        }
    }

    fn tool_result_block(id: &str, content: serde_json::Value) -> TuiContentBlock {
        TuiContentBlock::ToolResult {
            tool_use_id: id.to_string(),
            content,
            is_error: false,
            text: None,
        }
    }

    fn image_block() -> TuiContentBlock {
        TuiContentBlock::Image {
            media_type: "image/png".to_string(),
            base64: "iVBOR".to_string(),
            placeholder: Some("[Image #1]".to_string()),
        }
    }

    // ── parse: user 分支 ──

    #[test]
    fn test_parse_stop_hook_message_as_notice() {
        let mut message = msg(
            "user",
            vec![text_block("<system-reminder>blocked</system-reminder>")],
        );
        message.source = TuiMessageSource::StopHook;

        assert_eq!(
            HistoryDisplayMessage::parse(&message),
            Ok(HistoryDisplayMessage::StopHookNotice {
                body: "<system-reminder>blocked</system-reminder>".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_legacy_system_generated_stop_hook_message_as_notice() {
        let mut message = msg(
            "user",
            vec![text_block(
                "<system-reminder>Stop hook prevented stopping. Retry.</system-reminder>",
            )],
        );
        message.source = TuiMessageSource::SystemGenerated;

        assert!(matches!(
            HistoryDisplayMessage::parse(&message),
            Ok(HistoryDisplayMessage::StopHookNotice { .. })
        ));
    }

    #[test]
    fn test_parse_non_stop_system_generated_message_as_user() {
        let mut message = msg("user", vec![text_block("guidance changed")]);
        message.source = TuiMessageSource::SystemGenerated;

        assert_eq!(
            HistoryDisplayMessage::parse(&message),
            Ok(HistoryDisplayMessage::User {
                text: "guidance changed".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_user_text_only() {
        let m = msg("user", vec![text_block("hello")]);
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Ok(HistoryDisplayMessage::User {
                text: "hello".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_user_multiple_text_concatenated() {
        let m = msg("user", vec![text_block("hello "), text_block("world")]);
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Ok(HistoryDisplayMessage::User {
                text: "hello world".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_user_tool_result_only_becomes_tool_results() {
        let m = msg(
            "user",
            vec![tool_result_block("t1", serde_json::json!("done"))],
        );
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Ok(HistoryDisplayMessage::ToolResults)
        );
    }

    #[test]
    fn test_parse_user_text_with_tool_result_prefers_user() {
        let m = msg(
            "user",
            vec![
                text_block("question"),
                tool_result_block("t1", serde_json::json!("done")),
            ],
        );
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Ok(HistoryDisplayMessage::User {
                text: "question".to_string(),
            })
        );
    }

    #[test]
    fn test_parse_user_empty_returns_error() {
        let m = msg("user", vec![]);
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Err(HistoryDisplayParseError::EmptyUserText)
        );
    }

    #[test]
    fn test_parse_user_thinking_unsupported() {
        let m = msg("user", vec![thinking_block("hmm")]);
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Err(HistoryDisplayParseError::UnsupportedUserBlock(
                "thinking".to_string()
            ))
        );
    }

    /// #fix-tui-image-input-output：image block 现在按占位符 `[Image #N]` 渲染
    /// （保留 round-trip 位置），而非报错。
    #[test]
    fn test_parse_user_image_renders_placeholder() {
        let m = msg("user", vec![image_block()]);
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Ok(HistoryDisplayMessage::User {
                text: "[Image #1]".to_string()
            })
        );
    }

    // ── parse: assistant 分支 ──

    #[test]
    fn test_parse_assistant_text() {
        let m = msg("assistant", vec![text_block("answer")]);
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Ok(HistoryDisplayMessage::Assistant {
                blocks: vec![HistoryAssistantBlock::Text("answer".to_string())],
            })
        );
    }

    #[test]
    fn test_parse_assistant_thinking() {
        let m = msg("assistant", vec![thinking_block("plan")]);
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Ok(HistoryDisplayMessage::Assistant {
                blocks: vec![HistoryAssistantBlock::Thinking("plan".to_string())],
            })
        );
    }

    #[test]
    fn test_parse_assistant_tool_use() {
        let m = msg("assistant", vec![tool_use_block("t1", "Read")]);
        let Ok(HistoryDisplayMessage::Assistant { blocks }) = HistoryDisplayMessage::parse(&m)
        else {
            panic!("expected Assistant");
        };
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            HistoryAssistantBlock::ToolUse { id, name, .. } => {
                assert_eq!(id, "t1");
                assert_eq!(name, "Read");
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_assistant_mixed_blocks_preserves_order() {
        let m = msg(
            "assistant",
            vec![
                thinking_block("plan"),
                text_block("answer"),
                tool_use_block("t1", "Read"),
            ],
        );
        let Ok(HistoryDisplayMessage::Assistant { blocks }) = HistoryDisplayMessage::parse(&m)
        else {
            panic!("expected Assistant");
        };
        assert_eq!(blocks.len(), 3);
        assert!(matches!(&blocks[0], HistoryAssistantBlock::Thinking(t) if t == "plan"));
        assert!(matches!(&blocks[1], HistoryAssistantBlock::Text(t) if t == "answer"));
        assert!(
            matches!(&blocks[2], HistoryAssistantBlock::ToolUse { name, .. } if name == "Read")
        );
    }

    #[test]
    fn test_parse_assistant_tool_result_unsupported() {
        let m = msg(
            "assistant",
            vec![tool_result_block("t1", serde_json::json!("x"))],
        );
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Err(HistoryDisplayParseError::UnsupportedAssistantBlock(
                "tool_result".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_assistant_image_unsupported() {
        let m = msg("assistant", vec![image_block()]);
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Err(HistoryDisplayParseError::UnsupportedAssistantBlock(
                "image".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_assistant_empty_returns_error() {
        let m = msg("assistant", vec![]);
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Err(HistoryDisplayParseError::EmptyAssistantMessage)
        );
    }

    // ── parse: role 分支 ──

    #[test]
    fn test_parse_unknown_role_returns_error() {
        let m = msg("system", vec![text_block("notice")]);
        assert_eq!(
            HistoryDisplayMessage::parse(&m),
            Err(HistoryDisplayParseError::UnsupportedRole(
                "system".to_string()
            ))
        );
    }

    // ── collect_following_tool_results ──

    #[test]
    fn test_collect_following_tool_results_none_returns_empty() {
        let map = collect_following_tool_results(None);
        assert!(map.is_empty());
    }

    #[test]
    fn test_collect_following_tool_results_extracts_by_id() {
        let next = msg(
            "user",
            vec![
                text_block("ignored"),
                tool_result_block("t1", serde_json::json!("result-1")),
                tool_result_block("t2", serde_json::json!(["err"])),
            ],
        );
        let map = collect_following_tool_results(Some(&next));
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("t1"));
        assert!(map.contains_key("t2"));
    }

    // ── tool_result_display_text ──

    #[test]
    fn resumed_tool_result_prefers_persisted_text_for_every_tool() {
        for (tool_name, content) in [
            (
                "Bash",
                serde_json::json!({ "stdout": "raw bash", "exit_code": 0 }),
            ),
            ("Read", serde_json::json!({ "content": "raw read" })),
            ("Grep", serde_json::json!({ "matches": ["raw grep"] })),
            ("Agent", serde_json::json!({ "output": "raw agent" })),
            ("McpTool", serde_json::json!({ "value": "raw mcp" })),
        ] {
            assert_eq!(
                tool_result_display_text(HistoryToolResult {
                    content: &content,
                    text: Some("human display"),
                    is_error: false,
                }),
                "human display",
                "{tool_name} resume 必须统一优先使用持久化 text"
            );
        }
    }

    #[test]
    fn resumed_tool_result_without_text_falls_back_to_legacy_content() {
        let content = serde_json::json!({ "legacy": true });
        assert_eq!(
            tool_result_display_text(HistoryToolResult {
                content: &content,
                text: None,
                is_error: false,
            }),
            content.to_string()
        );
    }

    // ── tool_result_content_to_string ──

    #[test]
    fn test_tool_result_content_to_string_string_value() {
        assert_eq!(
            tool_result_content_to_string(&serde_json::json!("hello")),
            "hello"
        );
    }

    #[test]
    fn test_tool_result_content_to_string_array_joins_text_fields() {
        let content = serde_json::json!([
            { "type": "text", "text": "line1" },
            { "type": "text", "text": "line2" }
        ]);
        assert_eq!(tool_result_content_to_string(&content), "line1\nline2");
    }

    #[test]
    fn test_tool_result_content_to_string_object_falls_back_to_to_string() {
        let content = serde_json::json!({ "stdout": "out" });
        assert_eq!(tool_result_content_to_string(&content), content.to_string());
    }

    // ── normalize_tool_result_content ──

    #[test]
    fn test_normalize_string_wraps_in_text_object() {
        let result = normalize_tool_result_content(&serde_json::json!("raw"));
        assert_eq!(result, serde_json::json!({ "text": "raw" }));
    }

    #[test]
    fn test_normalize_array_joins_text_fields() {
        let content = serde_json::json!([
            { "type": "text", "text": "a" },
            { "type": "text", "text": "b" }
        ]);
        let result = normalize_tool_result_content(&content);
        assert_eq!(result, serde_json::json!({ "text": "a\nb" }));
    }

    // ── tool_result_image_count ──

    #[test]
    fn test_tool_result_image_count_counts_image_type() {
        let content = serde_json::json!([
            { "type": "text", "text": "x" },
            { "type": "image", "source": {} },
            { "type": "image", "source": {} }
        ]);
        assert_eq!(tool_result_image_count(&content), 2);
    }

    #[test]
    fn test_tool_result_image_count_non_array_returns_zero() {
        assert_eq!(tool_result_image_count(&serde_json::json!("text")), 0);
    }
}

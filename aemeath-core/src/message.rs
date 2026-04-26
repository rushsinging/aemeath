use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Describes a message integrity issue found during session validation.
#[derive(Debug, Clone, PartialEq)]
pub enum IntegrityIssue {
    /// ToolResult referencing a non-existent ToolUse (e.g., lost during compaction).
    OrphanedToolResult {
        msg_index: usize,
        tool_use_ids: Vec<String>,
    },
    /// Assistant message with tool_calls whose results are missing (not followed
    /// by matching user/ToolResult messages) and those results cannot
    /// be recovered from later messages.
    OrphanedToolUse {
        msg_index: usize,
        tool_ids: Vec<String>,
    },
    /// Back-to-back messages with the same role (user→user or assistant→assistant).
    RoleOrder {
        msg_index: usize,
        role: String,
    },
}

/// Results of a message integrity check.
#[derive(Debug, Clone, Default)]
pub struct IntegrityCheck {
    pub issues: Vec<IntegrityIssue>,
}

impl IntegrityCheck {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn has_issues(&self) -> bool {
        !self.issues.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
    },
    Thinking {
        #[serde(default)]
        thinking: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ImageSource {
    Base64 {
        media_type: String,
        data: String,
    },
}

/// Image dimensions for display and coordinate mapping
#[derive(Debug, Clone, Default)]
pub struct ImageDimensions {
    pub original_width: Option<u32>,
    pub original_height: Option<u32>,
    pub display_width: Option<u32>,
    pub display_height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    pub fn user_with_image(text: impl Into<String>, image_base64: String, media_type: String) -> Self {
        Self {
            role: Role::User,
            content: vec![
                ContentBlock::Image {
                    source: ImageSource::Base64 {
                        media_type,
                        data: image_base64,
                    },
                },
                ContentBlock::Text { text: text.into() },
            ],
        }
    }

    pub fn user_with_images(text: impl Into<String>, images: Vec<(String, String)>) -> Self {
        let mut content: Vec<ContentBlock> = images
            .into_iter()
            .map(|(data, media_type)| ContentBlock::Image {
                source: ImageSource::Base64 { media_type, data },
            })
            .collect();
        content.push(ContentBlock::Text { text: text.into() });
        Self {
            role: Role::User,
            content,
        }
    }

    pub fn tool_results(results: Vec<(String, String, bool)>) -> Self {
        Self {
            role: Role::User,
            content: results
                .into_iter()
                .map(|(tool_use_id, content, is_error)| ContentBlock::ToolResult {
                    tool_use_id,
                    content: serde_json::Value::String(content),
                    is_error,
                })
                .collect(),
        }
    }

    /// Create tool results with optional image attachments.
    /// Each result is (tool_use_id, text_content, is_error, images).
    pub fn tool_results_rich(results: Vec<(String, String, bool, Vec<crate::tool::ImageData>)>) -> Self {
        Self {
            role: Role::User,
            content: results
                .into_iter()
                .map(|(tool_use_id, text, is_error, images)| {
                    let content = if images.is_empty() {
                        serde_json::Value::String(text)
                    } else {
                        let mut blocks: Vec<serde_json::Value> = images
                            .into_iter()
                            .map(|img| serde_json::json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": img.media_type,
                                    "data": img.base64,
                                }
                            }))
                            .collect();
                        blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text,
                        }));
                        serde_json::Value::Array(blocks)
                    };
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    }
                })
                .collect(),
        }
    }

    pub fn extract_tool_uses(&self) -> Vec<(&str, &str, &serde_json::Value)> {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { id, name, input } => {
                    Some((id.as_str(), name.as_str(), input))
                }
                _ => None,
            })
            .collect()
    }

    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Returns true if this message contains any ToolUse blocks.
    pub fn has_tool_uses(&self) -> bool {
        self.content.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }))
    }

    /// Returns the ToolUse IDs in this message.
    pub fn tool_use_ids(&self) -> Vec<&str> {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Returns true if this message contains ToolResult blocks.
    pub fn has_tool_results(&self) -> bool {
        self.content.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }))
    }

    /// Returns the tool_use_ids of ToolResult blocks in this message.
    pub fn tool_result_ids(&self) -> Vec<&str> {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// Sanitize a message list so it is valid for the API.
///
/// The API requires that every assistant message with `tool_calls` (ToolUse blocks)
/// is immediately followed by a user message containing the corresponding ToolResult
/// blocks. If a session is interrupted mid-tool-call, this invariant is broken.
///
/// This function strips trailing orphaned ToolUse calls (and any ToolResult messages
/// that reference non-existent ToolUse IDs), ensuring the message list is always valid.
pub fn sanitize_messages(messages: &mut Vec<Message>) {
    // Strategy: walk through messages, track which tool_use_ids have been seen
    // but not yet answered. If we reach the end with unresolved tool calls,
    // strip the trailing assistant message(s) that contain them.

    let mut unresolved_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for msg in messages.iter() {
        if msg.role == Role::Assistant {
            for id in msg.tool_use_ids() {
                unresolved_ids.insert(id.to_string());
            }
        }
        if msg.role == Role::User {
            for id in msg.tool_result_ids() {
                unresolved_ids.remove(id);
            }
        }
    }

    // If no unresolved tool calls, nothing to do
    if unresolved_ids.is_empty() {
        return;
    }

    // Trim from the end: remove trailing messages that contain or reference unresolved tool calls
    while let Some(last) = messages.last() {
        let should_remove = match last.role {
            Role::Assistant => last.has_tool_uses(),
            Role::User => {
                // Remove tool result messages that reference unresolved calls
                // but keep pure text user messages
                last.has_tool_results()
            }
        };

        if should_remove {
            let removed = messages.pop().unwrap();
            // After removing, recalculate: the tool_use_ids from this removed assistant
            // message are no longer "seen", so remove them from unresolved set too
            if removed.role == Role::Assistant {
                for id in removed.tool_use_ids() {
                    unresolved_ids.remove(id);
                }
            }
        } else {
            break;
        }

        if unresolved_ids.is_empty() {
            break;
        }
    }
}

/// Check the integrity of a message list beyond what `sanitize_messages` catches.
///
/// This detects issues that cannot be fixed by trimming from the end alone:
///
/// - **OrphanedToolResult**: A ToolResult referencing a ToolUse ID that never appeared
///   in any prior assistant message. This can happen when compaction drops the
///   assistant message but keeps the tool result.
///
/// - **OrphanedToolUse**: A trailing assistant message with tool_calls whose results
///   are not present anywhere in the list, AND the list ends with that assistant
///   (i.e. it's not just waiting for the next user input).
///
/// - **RoleOrder**: Back-to-back messages with the same role, which violates
///   the user → assistant → user → assistant alternation expected by most APIs.
pub fn check_message_integrity(messages: &[Message]) -> IntegrityCheck {
    let mut issues = Vec::new();

    // --- Track all tool_use IDs that appeared in assistant messages ---
    let all_tool_use_ids: HashSet<&str> = messages
        .iter()
        .filter(|m| m.role == Role::Assistant)
        .flat_map(|m| m.tool_use_ids())
        .collect();

    // --- 1. Detect ToolResults that reference IDs that never appeared as ToolUse ---
    for (i, msg) in messages.iter().enumerate() {
        if msg.role == Role::User {
            let orphaned: Vec<String> = msg
                .tool_result_ids()
                .into_iter()
                .filter(|id| !all_tool_use_ids.contains(id))
                .map(|s| s.to_string())
                .collect();
            if !orphaned.is_empty() {
                issues.push(IntegrityIssue::OrphanedToolResult {
                    msg_index: i,
                    tool_use_ids: orphaned,
                });
            }
        }
    }

    // --- 2. Detect back-to-back same-role ---
    for i in 1..messages.len() {
        if messages[i].role == messages[i - 1].role {
            issues.push(IntegrityIssue::RoleOrder {
                msg_index: i,
                role: format!("{:?}", messages[i].role),
            });
        }
    }

    // --- 3. Detect trailing orphaned tool calls (the list ends with
    //         an assistant that has unfulfilled tool_calls) ---
    let mut pending_ids: HashSet<String> = HashSet::new();
    for msg in messages {
        if msg.role == Role::Assistant {
            for id in msg.tool_use_ids() {
                pending_ids.insert(id.to_string());
            }
        }
        if msg.role == Role::User {
            for id in msg.tool_result_ids() {
                pending_ids.remove(id);
            }
        }
    }
    if !pending_ids.is_empty() {
        // Find which assistant message(s) these belong to (from the end)
        for (i, msg) in messages.iter().enumerate().rev() {
            let current_pending = pending_ids.clone();
            let unresolved: Vec<String> = msg
                .tool_use_ids()
                .into_iter()
                .filter(|id| current_pending.contains(*id))
                .map(|s| s.to_string())
                .collect();
            if !unresolved.is_empty() {
                for id in &unresolved {
                    pending_ids.remove(id);
                }
                issues.push(IntegrityIssue::OrphanedToolUse {
                    msg_index: i,
                    tool_ids: unresolved,
                });
            }
            if pending_ids.is_empty() {
                break;
            }
        }
    }

    IntegrityCheck { issues }
}

/// Deep-clean a message list by removing orphaned (unfixable) messages.
///
/// Goes beyond `sanitize_messages`:
/// - Removes ToolResult messages in the **middle** of the list that reference
///   non-existent ToolUse IDs (common after compaction splits a pair).
/// - Removes trailing orphaned tool-call messages (same as sanitize_messages).
///
/// Returns the number of messages removed.
pub fn deep_clean_messages(messages: &mut Vec<Message>) -> usize {
    let check = check_message_integrity(messages);
    if check.is_clean() {
        return 0_usize;
    }

    let mut removed = 0_usize;

    // Phase 1: Remove orphaned tool results (from end to avoid index shifting)
    // We collect them first, then remove from the end in reverse order.
    let mut to_remove: Vec<usize> = Vec::new();
    for issue in &check.issues {
        if let IntegrityIssue::OrphanedToolResult { msg_index, .. } = issue {
            to_remove.push(*msg_index);
        }
    }
    to_remove.sort_unstable();
    to_remove.dedup();
    for idx in to_remove.into_iter().rev() {
        messages.remove(idx);
        removed += 1;
    }

    // Phase 2: Fix role-ordering issues by inserting empty placeholders
    // We iterate from the end to avoid index offsets.
    let mut i = messages.len();
    while i > 1 {
        i -= 1;
        if messages[i].role == messages[i - 1].role {
            // Insert a minimal text-only message of the opposite role
            let placeholder = match messages[i].role {
                Role::User => Message {
                    role: Role::Assistant,
                    content: vec![ContentBlock::Text {
                        text: "(continued)".to_string(),
                    }],
                },
                Role::Assistant => Message {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "(continued)".to_string(),
                    }],
                },
            };
            messages.insert(i, placeholder);
            removed += 1;
        }
    }

    // Phase 3: Run standard sanitize for trailing orphaned tool calls
    let before = messages.len();
    sanitize_messages(messages);
    removed += before - messages.len();

    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper constructors ────────────────────────────────────────

    /// Build an assistant message with ToolUse blocks.
    fn assistant_with_tools(ids: &[&str]) -> Message {
        Message {
            role: Role::Assistant,
            content: ids
                .iter()
                .map(|&id| ContentBlock::ToolUse {
                    id: id.to_string(),
                    name: "Bash".to_string(),
                    input: serde_json::json!({}),
                })
                .collect(),
        }
    }

    /// Build an assistant message that is purely text.
    fn assistant_text(text: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        }
    }

    // ── 1. Message constructors ────────────────────────────────────

    #[test]
    fn test_user_constructor() {
        let msg = Message::user("hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content.len(), 1);
        assert!(matches!(
            &msg.content[0],
            ContentBlock::Text { text } if text == "hello"
        ));
    }

    #[test]
    fn test_user_with_image() {
        let msg = Message::user_with_image("caption", "base64data".into(), "image/png".into());
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content.len(), 2);
        // First block should be Image
        assert!(matches!(
            &msg.content[0],
            ContentBlock::Image {
                source: ImageSource::Base64 { media_type, data },
            } if media_type == "image/png" && data == "base64data"
        ));
        // Second block should be Text
        assert!(matches!(
            &msg.content[1],
            ContentBlock::Text { text } if text == "caption"
        ));
    }

    #[test]
    fn test_user_with_images_multiple() {
        let images = vec![
            ("img1".to_string(), "image/png".to_string()),
            ("img2".to_string(), "image/jpeg".to_string()),
        ];
        let msg = Message::user_with_images("multi caption", images);
        assert_eq!(msg.role, Role::User);
        // 2 Image blocks + 1 Text block = 3
        assert_eq!(msg.content.len(), 3);
        assert!(matches!(&msg.content[0], ContentBlock::Image { .. }));
        assert!(matches!(&msg.content[1], ContentBlock::Image { .. }));
        assert!(matches!(
            &msg.content[2],
            ContentBlock::Text { text } if text == "multi caption"
        ));
    }

    #[test]
    fn test_tool_results_constructor() {
        let msg = Message::tool_results(vec![
            ("tool_1".to_string(), "result1".to_string(), false),
            ("tool_2".to_string(), "error!".to_string(), true),
        ]);
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content.len(), 2);

        match &msg.content[0] {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tool_1");
                assert_eq!(content, &serde_json::Value::String("result1".to_string()));
                assert!(!is_error);
            }
            _ => panic!("expected ToolResult"),
        }

        match &msg.content[1] {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tool_2");
                assert!(content.as_str().unwrap_or_default().contains("error"));
                assert!(is_error);
            }
            _ => panic!("expected ToolResult"),
        }
    }

    // ── 2. Query methods ───────────────────────────────────────────

    #[test]
    fn test_extract_tool_uses_present() {
        let msg = assistant_with_tools(&["t1", "t2"]);
        let uses = msg.extract_tool_uses();
        assert_eq!(uses.len(), 2);
        assert_eq!(uses[0].0, "t1");
        assert_eq!(uses[0].1, "Bash");
        assert_eq!(uses[1].0, "t2");
    }

    #[test]
    fn test_extract_tool_uses_empty() {
        let msg = Message::user("no tools here");
        assert!(msg.extract_tool_uses().is_empty());
    }

    #[test]
    fn test_text_content_single() {
        let msg = Message::user("hello world");
        assert_eq!(msg.text_content(), "hello world");
    }

    #[test]
    fn test_text_content_multiple_blocks() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "part1".to_string(),
                },
                ContentBlock::Text {
                    text: "part2".to_string(),
                },
            ],
        };
        assert_eq!(msg.text_content(), "part1part2");
    }

    #[test]
    fn test_text_content_skips_non_text() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "before".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "x".to_string(),
                    name: "Y".to_string(),
                    input: serde_json::json!({}),
                },
                ContentBlock::Text {
                    text: "after".to_string(),
                },
            ],
        };
        assert_eq!(msg.text_content(), "beforeafter");
    }

    #[test]
    fn test_has_tool_uses_and_tool_results() {
        let text_msg = Message::user("hi");
        assert!(!text_msg.has_tool_uses());
        assert!(!text_msg.has_tool_results());

        let tool_msg = assistant_with_tools(&["a"]);
        assert!(tool_msg.has_tool_uses());
        assert!(!tool_msg.has_tool_results());

        let result_msg = Message::tool_results(vec![("a".into(), "ok".into(), false)]);
        assert!(!result_msg.has_tool_uses());
        assert!(result_msg.has_tool_results());
    }

    #[test]
    fn test_tool_use_ids() {
        let msg = assistant_with_tools(&["id_a", "id_b"]);
        assert_eq!(msg.tool_use_ids(), vec!["id_a", "id_b"]);
    }

    #[test]
    fn test_tool_result_ids() {
        let msg = Message::tool_results(vec![
            ("r1".into(), "ok".into(), false),
            ("r2".into(), "ok".into(), false),
        ]);
        assert_eq!(msg.tool_result_ids(), vec!["r1", "r2"]);
    }

    // ── 3. sanitize_messages ───────────────────────────────────────

    #[test]
    fn test_sanitize_clean_messages_unchanged() {
        let mut msgs = vec![
            Message::user("hello"),
            assistant_text("hi there"),
            Message::user("how are you"),
        ];
        let len_before = msgs.len();
        sanitize_messages(&mut msgs);
        assert_eq!(msgs.len(), len_before);
    }

    #[test]
    fn test_sanitize_removes_trailing_orphaned_tool_use() {
        let mut msgs = vec![
            Message::user("go"),
            assistant_with_tools(&["tool_1"]),
            // no user message with ToolResult for tool_1
        ];
        sanitize_messages(&mut msgs);
        // The trailing assistant with ToolUse should be removed
        assert!(msgs.last().unwrap().role == Role::User);
        assert_eq!(msgs.len(), 1);
    }

    #[test]
    fn test_sanitize_removes_trailing_orphaned_tool_result() {
        // When the trailing message is a ToolResult referencing a ToolUse that
        // *does* exist earlier, but there's no unfulfilled tool call, sanitize
        // leaves it alone.  The real cleanup of orphaned ToolResult (no matching
        // ToolUse at all) is handled by `deep_clean_messages`.
        //
        // Instead, test the case where an assistant has a ToolUse and is followed
        // only by a ToolResult for a *different* (non-existent) tool.
        let mut msgs = vec![
            Message::user("start"),
            assistant_with_tools(&["tool_1"]),
            // This ToolResult references a ghost tool, NOT tool_1.
            // tool_1 is still unresolved, so sanitize will strip from the end.
            Message::tool_results(vec![("ghost_tool".into(), "result".into(), false)]),
        ];
        sanitize_messages(&mut msgs);
        // Both the ToolResult msg and the assistant with unresolved tool_1 are removed
        assert!(msgs.last().unwrap().role == Role::User);
    }

    #[test]
    fn test_sanitize_preserves_complete_tool_pairs_in_middle() {
        let mut msgs = vec![
            Message::user("start"),
            assistant_with_tools(&["tool_1"]),
            Message::tool_results(vec![("tool_1".into(), "ok".into(), false)]),
            assistant_text("final"),
        ];
        let len_before = msgs.len();
        sanitize_messages(&mut msgs);
        assert_eq!(msgs.len(), len_before);
    }

    // ── 4. check_message_integrity ─────────────────────────────────

    #[test]
    fn test_integrity_clean() {
        let msgs = vec![
            Message::user("hi"),
            assistant_text("hello"),
            Message::user("go"),
        ];
        let check = check_message_integrity(&msgs);
        assert!(check.is_clean());
        assert!(!check.has_issues());
    }

    #[test]
    fn test_integrity_detects_orphaned_tool_result() {
        let msgs = vec![
            Message::user("hi"),
            assistant_text("hello"),
            // ToolResult referencing a tool_use_id that never appeared
            Message::tool_results(vec![("nonexistent".into(), "data".into(), false)]),
        ];
        let check = check_message_integrity(&msgs);
        assert!(check.has_issues());
        let found = check
            .issues
            .iter()
            .any(|i| matches!(i, IntegrityIssue::OrphanedToolResult { .. }));
        assert!(found, "expected OrphanedToolResult issue");
    }

    #[test]
    fn test_integrity_detects_orphaned_tool_use() {
        let msgs = vec![
            Message::user("go"),
            assistant_with_tools(&["tool_x"]),
            // No matching ToolResult — ends with orphaned tool call
        ];
        let check = check_message_integrity(&msgs);
        assert!(check.has_issues());
        let found = check
            .issues
            .iter()
            .any(|i| matches!(i, IntegrityIssue::OrphanedToolUse { .. }));
        assert!(found, "expected OrphanedToolUse issue");
    }

    #[test]
    fn test_integrity_detects_role_order() {
        let msgs = vec![
            Message::user("first"),
            Message::user("second"), // back-to-back User — bad
            assistant_text("reply"),
        ];
        let check = check_message_integrity(&msgs);
        assert!(check.has_issues());
        let found = check
            .issues
            .iter()
            .any(|i| matches!(i, IntegrityIssue::RoleOrder { .. }));
        assert!(found, "expected RoleOrder issue");
    }

    // ── 5. deep_clean_messages ─────────────────────────────────────

    #[test]
    fn test_deep_clean_removes_mid_orphaned_tool_result() {
        let mut msgs: Vec<Message> = vec![
            Message::user("start"),
            assistant_text("reply"),
            // Orphaned ToolResult in the middle (no matching ToolUse)
            Message::tool_results(vec![("orphan".into(), "x".into(), false)]),
            assistant_text("final"),
            Message::user("end"),
        ];
        let removed = deep_clean_messages(&mut msgs);
        assert!(removed > 0, "should have removed at least one message");
        // The orphaned ToolResult message should be gone
        assert!(msgs
            .iter()
            .all(|m| !m.has_tool_results() || m.tool_result_ids().into_iter().any(|id| id != "orphan")));
    }

    #[test]
    fn test_deep_clean_fixes_role_order_with_placeholder() {
        let mut msgs = vec![
            Message::user("first"),
            Message::user("second"), // back-to-back User
            assistant_text("reply"),
        ];
        let _removed = deep_clean_messages(&mut msgs);
        // A placeholder message was inserted, so "removed" counts it as an insertion
        // (the function counts placeholders in its `removed` counter but the list grows)
        // Verify roles now alternate
        for pair in msgs.windows(2) {
            assert_ne!(pair[0].role, pair[1].role);
        }
    }

    #[test]
    fn test_deep_clean_handles_trailing_orphaned_tool_use() {
        let mut msgs = vec![
            Message::user("go"),
            assistant_with_tools(&["tool_1"]),
        ];
        let removed = deep_clean_messages(&mut msgs);
        assert!(removed > 0);
        // Trailing orphaned ToolUse assistant should have been removed
        assert!(
            msgs.last().unwrap().role == Role::User,
            "last message should be the user message"
        );
    }

    #[test]
    fn test_deep_clean_clean_messages_noop() {
        let mut msgs = vec![
            Message::user("hi"),
            assistant_text("hello"),
            Message::user("go"),
        ];
        let removed = deep_clean_messages(&mut msgs);
        assert_eq!(removed, 0);
        assert_eq!(msgs.len(), 3);
    }

    // ── 6. IntegrityCheck helpers ──────────────────────────────────

    #[test]
    fn test_integrity_check_default_is_clean() {
        let check = IntegrityCheck::default();
        assert!(check.is_clean());
        assert!(!check.has_issues());
        assert!(check.issues.is_empty());
    }

    #[test]
    fn test_integrity_check_with_issues() {
        let check = IntegrityCheck {
            issues: vec![IntegrityIssue::RoleOrder {
                msg_index: 1,
                role: "User".to_string(),
            }],
        };
        assert!(!check.is_clean());
        assert!(check.has_issues());
    }
}

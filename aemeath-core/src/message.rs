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

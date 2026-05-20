//! 消息完整性与清理
//!
//! 包含 sanitize_messages, check_message_integrity, deep_clean_messages。

use crate::message::types::*;
use std::collections::HashSet;

/// 计算消息列表中待完成的 tool_use_id 集合。
///
/// 遍历所有消息，Assistant 消息添加新 ToolUse id，User 消息移除已完成的 ToolResult id。
fn pending_tool_use_ids(messages: &[Message]) -> HashSet<String> {
    let mut pending: HashSet<String> = HashSet::new();
    for msg in messages {
        if msg.role == Role::Assistant {
            for id in msg.tool_use_ids() {
                pending.insert(id.to_string());
            }
        }
        if msg.role == Role::User {
            for id in msg.tool_result_ids() {
                pending.remove(id);
            }
        }
    }
    pending
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
    let mut unresolved_ids = pending_tool_use_ids(messages);

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
    let mut pending_ids = pending_tool_use_ids(messages);
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
    let mut i = messages.len();
    while i > 1 {
        i -= 1;
        if messages[i].role == messages[i - 1].role {
            let placeholder = Message::placeholder(messages[i].role.opposite());
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

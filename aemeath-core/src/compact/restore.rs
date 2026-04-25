//! 压缩后文件恢复与消息重组
//!
//! 压缩完成后，恢复最近读取的文件内容以保持上下文连贯；
//! 清理孤立工具调用对；重组最终消息列表。

use std::collections::HashSet;

use crate::message::{ContentBlock, Message, Role};
use crate::token_estimation::estimate_tokens;

/// 压缩后恢复的最大最近读取文件数。
pub const POST_COMPACT_MAX_FILES: usize = 5;

/// 每个恢复文件的最大 token 数。
pub const POST_COMPACT_MAX_TOKENS_PER_FILE: usize = 5_000;

/// 所有恢复文件的总 token 预算。
pub const POST_COMPACT_TOKEN_BUDGET: usize = 50_000;

/// 从最近读取的文件路径集合构建文件恢复附件。
/// 按修改时间排序读取最新的文件（不超过预算），返回要注入的摘要消息。
pub fn build_file_restoration(read_files: &HashSet<String>) -> Option<String> {
    if read_files.is_empty() {
        return None;
    }

    // 收集文件及其修改时间，按最近优先排序
    let mut files_with_mtime: Vec<(String, std::time::SystemTime)> = read_files
        .iter()
        .filter_map(|path| {
            let metadata = std::fs::metadata(path).ok()?;
            let mtime = metadata.modified().ok()?;
            Some((path.clone(), mtime))
        })
        .collect();

    files_with_mtime.sort_by(|a, b| b.1.cmp(&a.1));

    let mut restored_content = String::new();
    let mut total_tokens = 0usize;
    let mut file_count = 0usize;

    for (path, _mtime) in files_with_mtime.iter().take(POST_COMPACT_MAX_FILES) {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let file_tokens = estimate_tokens(&content);
        let truncated = if file_tokens > POST_COMPACT_MAX_TOKENS_PER_FILE {
            let max_chars = POST_COMPACT_MAX_TOKENS_PER_FILE * 4; // ~4 字符/token
            let end = max_chars.min(content.len());
            let mut boundary = end;
            while boundary > 0 && !content.is_char_boundary(boundary) {
                boundary -= 1;
            }
            format!("{}...\n[truncated, {} total chars]", &content[..boundary], content.len())
        } else {
            content
        };

        let entry_tokens = estimate_tokens(&truncated) + 20; // 标签开销
        if total_tokens + entry_tokens > POST_COMPACT_TOKEN_BUDGET {
            break;
        }

        restored_content.push_str(&format!(
            "\n<file path=\"{path}\">\n{truncated}\n</file>\n"
        ));
        total_tokens += entry_tokens;
        file_count += 1;
    }

    if file_count == 0 {
        return None;
    }

    Some(format!(
        "<system-reminder>\n[Post-compaction file restoration: {} recently-read files]\n{restored_content}\n</system-reminder>",
        file_count
    ))
}

/// 清理压缩/截断后孤立的 ToolUse / ToolResult 对。
///
/// OpenAI 严格要求："assistant message with 'tool_calls' must be followed by
/// tool messages responding to each tool_call_id"。一旦 tool_use 没紧跟对应
/// tool_result（compact 切断、用户中途中断、工具执行失败等），后续 API 调用
/// 就会 400。本函数就地修复：
///
/// - 为每个 `assistant.tool_use` 集合 S，紧跟着的"工具消息块"必须覆盖 S 全部 id
/// - 缺失的 id 立即在该 assistant 之后插入一个 user 占位消息补齐
/// - 与任何 assistant.tool_use 都不匹配的孤立 tool_result 被移除
pub fn sanitize_tool_pairs(messages: &mut Vec<Message>) {
    // 1) 收集所有 assistant 消息发出过的 tool_use_id（用于识别孤立 result）
    let known_tool_use_ids: HashSet<String> = messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            ContentBlock::ToolUse { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();

    // 2) 移除孤立的 tool_result（其对应的 tool_use 已被 compact 切掉）
    for msg in messages.iter_mut() {
        msg.content.retain(|block| {
            if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                known_tool_use_ids.contains(tool_use_id)
            } else {
                true
            }
        });
    }
    // 顺手剔除被 retain 后变空的消息
    messages.retain(|m| !m.content.is_empty());

    // 3) 对每条带 tool_use 的 assistant 消息，确保紧跟的 user 消息覆盖全部
    //    tool_use_id。缺哪个就立即补一个占位 tool_result。
    let mut i = 0;
    while i < messages.len() {
        let pending_ids: Vec<String> = if messages[i].role == Role::Assistant {
            messages[i]
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };

        if pending_ids.is_empty() {
            i += 1;
            continue;
        }

        // 收集紧跟 assistant 的连续 user 消息中已经存在的 tool_result_id
        let mut have_ids: HashSet<String> = HashSet::new();
        let mut last_user_idx: Option<usize> = None;
        let mut j = i + 1;
        while j < messages.len() && messages[j].role == Role::User {
            for block in &messages[j].content {
                if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                    have_ids.insert(tool_use_id.clone());
                }
            }
            last_user_idx = Some(j);
            j += 1;
            // 注意：不要跨过下一条 assistant；连续 user 才算"紧跟"
        }

        // 哪些 tool_use 缺 tool_result
        let missing: Vec<String> = pending_ids
            .into_iter()
            .filter(|id| !have_ids.contains(id))
            .collect();

        if !missing.is_empty() {
            let placeholder_blocks: Vec<ContentBlock> = missing
                .into_iter()
                .map(|id| ContentBlock::ToolResult {
                    tool_use_id: id,
                    content: serde_json::json!(
                        "[result removed during compaction]"
                    ),
                    is_error: false,
                })
                .collect();

            if let Some(uidx) = last_user_idx {
                // 已有紧跟的 user 消息，在它的 content 前面追加占位
                // （放前面也行，OpenAI 不在意单 user 消息内部 tool_result 顺序，
                //  只要全部 tool_call_id 都被覆盖即可）
                let mut new_content = placeholder_blocks;
                new_content.append(&mut messages[uidx].content);
                messages[uidx].content = new_content;
                i = uidx + 1;
            } else {
                // 紧跟的不是 user（可能是另一条 assistant 或者已经到末尾）
                // 立即在 assistant 之后插入一条 user 消息承载占位 tool_result
                let placeholder_msg = Message {
                    role: Role::User,
                    content: placeholder_blocks,
                };
                messages.insert(i + 1, placeholder_msg);
                i += 2;
            }
        } else {
            i = last_user_idx.map(|u| u + 1).unwrap_or(i + 1);
        }
    }
}

/// 从摘要 + 近期消息组装最终压缩结果。
pub fn assemble_compacted(
    summary: String,
    recent_messages: &[Message],
    original_early_count: usize,
) -> (Vec<Message>, bool) {
    assemble_compacted_with_files(summary, recent_messages, original_early_count, None)
}

/// 从摘要 + 近期消息组装最终压缩结果（带可选文件恢复）。
pub fn assemble_compacted_with_files(
    summary: String,
    recent_messages: &[Message],
    original_early_count: usize,
    read_files: Option<&HashSet<String>>,
) -> (Vec<Message>, bool) {
    let mut compacted = Vec::with_capacity(recent_messages.len() + 4);

    // 摘要消息
    let mut summary_text = format!(
        "<system-reminder>\n[Conversation summary of {} earlier messages]\n{}\n</system-reminder>",
        original_early_count, summary
    );

    // 附加文件恢复内容
    if let Some(files) = read_files {
        if let Some(restoration) = build_file_restoration(files) {
            summary_text.push_str("\n\n");
            summary_text.push_str(&restoration);
        }
    }

    compacted.push(Message {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: summary_text,
        }],
    });

    compacted.push(Message {
        role: Role::Assistant,
        content: vec![ContentBlock::Text {
            text: "Understood. I have the context from our earlier conversation. Let me continue."
                .to_string(),
        }],
    });

    for msg in recent_messages {
        compacted.push(msg.clone());
    }

    fix_role_alternation(&mut compacted);
    sanitize_tool_pairs(&mut compacted);
    fix_role_alternation(&mut compacted); // sanitize_tool_pairs 可能追加了占位符，需再次修复角色交替
    (compacted, true)
}

/// 确保消息在 User / Assistant 角色之间交替。
pub fn fix_role_alternation(messages: &mut Vec<Message>) {
    let mut i = 1;
    while i < messages.len() {
        if messages[i].role == messages[i - 1].role {
            let filler_role = match messages[i].role {
                Role::User => Role::Assistant,
                Role::Assistant => Role::User,
            };
            let filler = Message {
                role: filler_role,
                content: vec![ContentBlock::Text {
                    text: "(continued)".to_string(),
                }],
            };
            messages.insert(i, filler);
            i += 1;
        }
        i += 1;
    }
}

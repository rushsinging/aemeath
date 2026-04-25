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

/// 清理压缩后孤立的 ToolUse / ToolResult 对。
pub fn sanitize_tool_pairs(messages: &mut Vec<Message>) {
    let mut tool_use_ids: HashSet<String> = HashSet::new();
    let mut tool_result_ids: HashSet<String> = HashSet::new();

    for msg in messages.iter() {
        for block in &msg.content {
            match block {
                ContentBlock::ToolUse { id, .. } => {
                    tool_use_ids.insert(id.clone());
                }
                ContentBlock::ToolResult { tool_use_id, .. } => {
                    tool_result_ids.insert(tool_use_id.clone());
                }
                _ => {}
            }
        }
    }

    // 移除没有匹配 ToolUse 的孤立 ToolResult
    let orphan_results: HashSet<&String> =
        tool_result_ids.difference(&tool_use_ids).collect();
    if !orphan_results.is_empty() {
        for msg in messages.iter_mut() {
            msg.content.retain(|block| {
                if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                    !orphan_results.contains(tool_use_id)
                } else {
                    true
                }
            });
        }
    }

    // 为没有结果的 ToolUse 添加占位结果
    let missing_results: Vec<String> = tool_use_ids
        .difference(&tool_result_ids)
        .cloned()
        .collect();
    if !missing_results.is_empty() {
        let placeholder_msg = Message {
            role: Role::User,
            content: missing_results
                .into_iter()
                .map(|id| ContentBlock::ToolResult {
                    tool_use_id: id,
                    content: serde_json::json!("[result removed during compaction]"),
                    is_error: false,
                })
                .collect(),
        };
        let insert_pos = if messages.is_empty() { 0 } else { messages.len() - 1 };
        messages.insert(insert_pos, placeholder_msg);
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

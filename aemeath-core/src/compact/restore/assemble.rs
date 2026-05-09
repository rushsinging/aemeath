//! 压缩结果组装与角色交替修复

use crate::compact::restore::restore_files::build_file_restoration;
use crate::compact::restore::sanitize_pairs::sanitize_tool_pairs;
use crate::message::{ContentBlock, Message, Role};
use std::collections::HashSet;

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
        content: vec![ContentBlock::Text { text: summary_text }],
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

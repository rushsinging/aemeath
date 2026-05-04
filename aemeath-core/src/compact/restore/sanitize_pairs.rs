//! ToolUse/ToolResult 配对清理
//!
//! OpenAI 严格要求："assistant message with 'tool_calls' must be followed by
//! tool messages responding to each tool_call_id"。一旦 tool_use 没紧跟对应
//! tool_result（compact 切断、用户中途中断、工具执行失败等），后续 API 调用
//! 就会 400。本函数就地修复：
//!
//! - 为每个 `assistant.tool_use` 集合 S，紧跟着的"工具消息块"必须覆盖 S 全部 id
//! - 缺失的 id 立即在该 assistant 之后插入一个 user 占位消息补齐
//! - 与任何 assistant.tool_use 都不匹配的孤立 tool_result 被移除

use std::collections::HashSet;
use crate::message::{ContentBlock, Message, Role};

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
                    content: serde_json::json!("[result removed during compaction]"),
                    is_error: false,
                })
                .collect();

            if let Some(uidx) = last_user_idx {
                // 已有紧跟的 user 消息，在它的 content 前面追加占位
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

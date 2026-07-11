//! Microcompact：规则驱动清理陈旧探索类工具结果
//!
//! 在 auto-compact（LLM 摘要）之前、budget reduction 之后运行。
//! 纯规则判断，不调用 LLM，运行成本接近零。
//!
//! 两种入口：
//! - **主循环**：`microcompact_chain(&mut ChatChain)` — 按 segment 边界保护最近 3 个大 loop。
//! - **sub-agent**：`microcompact_messages(&mut [Message])` — 按 User turn 保护最近 2 轮。

use crate::business::session::ChatChain;
use share::message::{ContentBlock, Message, MessageSource, Role};
use std::collections::HashMap;

/// 探索类/只读工具白名单——这些工具的结果"过期"后不再有参考价值。
pub const EXPLORATORY_TOOLS: &[&str] = &[
    "Read",
    "Grep",
    "Glob",
    "Bash",
    "WebFetch",
    "WebSearch",
    "LS",
    "ToolSearch",
];

/// 主循环：保护最近 N 个 segment 的探索类 ToolResult 不被清理。
const PROTECT_RECENT_SEGMENTS: usize = 3;

/// Sub-agent：保护最近 N 轮的 ToolResult 不被清理。
/// "轮"以真实 User message 为分界（排除 ToolResult / 系统注入）。
const PROTECT_RECENT_TURNS: usize = 2;

/// 占位符模板。
const PLACEHOLDER_TEMPLATE: &str = "[Old tool result cleared (was {n} chars)]";

// ── 主循环入口 ──────────────────────────────────────

/// 对 ChatChain 执行 microcompact：保护最近 `PROTECT_RECENT_SEGMENTS` 个 segment，
/// 折叠更早 segment 中的探索类 ToolResult 为占位符。
///
/// 返回被清理的 ToolResult 数量。
pub fn microcompact_chain(chain: &mut ChatChain) -> usize {
    let segments = chain.active_segments();
    if segments.len() <= PROTECT_RECENT_SEGMENTS {
        return 0;
    }

    // 建立 tool_use_id → tool_name 映射（全链扫描）
    let tool_names = build_tool_name_map_flat(chain);

    let mut cleared = 0;
    let protect_from_seg = segments.len() - PROTECT_RECENT_SEGMENTS;

    for (seg_idx, seg) in chain.active_segments_mut().iter_mut().enumerate() {
        if seg_idx >= protect_from_seg {
            break;
        }
        for msg in &mut seg.messages {
            for block in &mut msg.content {
                cleared += clear_if_exploratory(block, &tool_names);
            }
        }
    }

    cleared
}

// ── sub-agent 入口 ──────────────────────────────────

/// 对扁平 messages 执行 microcompact（供 sub-agent 使用）。
///
/// 修复 turn 检测：用 `is_real_user_turn` 替代裸 `Role::User` 计数，
/// 避免 ToolResult / 系统注入被误判为 turn 边界。
///
/// 返回被清理的 ToolResult 数量。不改变 messages 的长度。
pub fn microcompact_messages(messages: &mut [Message]) -> usize {
    if messages.len() <= 4 {
        return 0;
    }

    let tool_names = build_tool_name_map(messages);
    let protect_from = protect_from_index(messages);

    let mut cleared = 0;
    for (i, msg) in messages.iter_mut().enumerate() {
        if i >= protect_from {
            break;
        }
        for block in &mut msg.content {
            cleared += clear_if_exploratory(block, &tool_names);
        }
    }

    cleared
}

// ── 共享逻辑 ──────────────────────────────────────

/// 判断工具是否属于探索类（可被 microcompact 清理）。
fn is_exploratory(tool_name: &str) -> bool {
    EXPLORATORY_TOOLS.contains(&tool_name)
}

/// 判断一条消息是否为真实用户 turn（排除 ToolResult 批次和系统注入）。
fn is_real_user_turn(msg: &Message) -> bool {
    if !matches!(msg.role, Role::User) {
        return false;
    }
    if msg
        .content
        .iter()
        .all(|b| matches!(b, ContentBlock::ToolResult { .. }))
    {
        return false;
    }
    if msg
        .metadata
        .as_ref()
        .map(|m| m.source == MessageSource::SystemGenerated)
        .unwrap_or(false)
    {
        return false;
    }
    true
}

/// 如果 block 是探索类 ToolResult，替换为占位符，返回 1；否则返回 0。
fn clear_if_exploratory(block: &mut ContentBlock, tool_names: &HashMap<String, String>) -> usize {
    let ContentBlock::ToolResult {
        tool_use_id,
        content,
        text,
        ..
    } = block
    else {
        return 0;
    };

    let tool_name = tool_names.get(tool_use_id.as_str());
    let Some(name) = tool_name else {
        return 0; // 未知工具名，保守保留
    };
    if !is_exploratory(name) {
        return 0; // 非探索类，保留
    }

    let original_size = tool_result_size(content, text.as_deref());
    if original_size == 0 {
        return 0;
    }

    let placeholder = PLACEHOLDER_TEMPLATE.replace("{n}", &original_size.to_string());
    *content = serde_json::Value::String(placeholder.clone());
    *text = Some(placeholder);
    1
}

/// 从所有消息中建立 tool_use_id → tool_name 映射（owned，避免借用冲突）。
fn build_tool_name_map(messages: &[Message]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for msg in messages {
        for block in &msg.content {
            if let ContentBlock::ToolUse { id, name, .. } = block {
                map.insert(id.clone(), name.clone());
            }
        }
    }
    map
}

/// 从 ChatChain 全部 segment 建立 tool_use_id → tool_name 映射。
fn build_tool_name_map_flat(chain: &ChatChain) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for seg in chain.active_segments() {
        for msg in &seg.messages {
            for block in &msg.content {
                if let ContentBlock::ToolUse { id, name, .. } = block {
                    map.insert(id.clone(), name.clone());
                }
            }
        }
    }
    map
}

/// 计算保护边界的 message index（sub-agent 用）。
///
/// 从末尾向前数 `PROTECT_RECENT_TURNS` 个真实 user turn，
/// 最后一个 real user turn 的 index 就是保护边界。
fn protect_from_index(messages: &[Message]) -> usize {
    let mut user_count = 0;
    for (i, msg) in messages.iter().enumerate().rev() {
        if is_real_user_turn(msg) {
            user_count += 1;
            if user_count == PROTECT_RECENT_TURNS {
                return i;
            }
        }
    }
    0
}

/// 计算 ToolResult 的内容大小（用于占位符显示）。
fn tool_result_size(content: &serde_json::Value, text: Option<&str>) -> usize {
    if let Some(t) = text {
        return t.len();
    }
    match content {
        serde_json::Value::String(s) => s.len(),
        _ => content.to_string().len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn user_msg(text: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            metadata: None,
        }
    }

    fn assistant_msg(text: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            metadata: None,
        }
    }

    fn tool_use_msg(id: &str, name: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: id.to_string(),
                name: name.to_string(),
                input: json!({}),
            }],
            metadata: None,
        }
    }

    fn tool_result_msg(id: &str, content: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: serde_json::Value::String(content.to_string()),
                is_error: false,
                text: Some(content.to_string()),
            }],
            metadata: None,
        }
    }

    fn tool_result_msg_no_text(id: &str, content: &str) -> Message {
        Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: serde_json::Value::String(content.to_string()),
                is_error: false,
                text: None,
            }],
            metadata: None,
        }
    }

    // ── is_exploratory ──────────────────────────────────

    #[test]
    fn test_is_exploratory_whitelist() {
        assert!(is_exploratory("Read"));
        assert!(is_exploratory("Grep"));
        assert!(is_exploratory("Bash"));
        assert!(is_exploratory("WebFetch"));
    }

    #[test]
    fn test_is_exploratory_not_whitelist() {
        assert!(!is_exploratory("Edit"));
        assert!(!is_exploratory("Write"));
        assert!(!is_exploratory("TaskCreate"));
        assert!(!is_exploratory("UnknownTool"));
    }

    // ── protect_from_index ──────────────────────────────

    #[test]
    fn test_protect_from_index_enough_turns() {
        // 3 个 User message → protect_from 是倒数第 2 个
        let msgs = vec![
            user_msg("turn1"), // 0
            assistant_msg("a"),
            user_msg("turn2"), // 2
            assistant_msg("b"),
            user_msg("turn3"), // 4
        ];
        // 倒数第 2 个 User = index 2
        assert_eq!(protect_from_index(&msgs), 2);
    }

    #[test]
    fn test_protect_from_index_few_turns() {
        // 只有 1 个 User → 保护全部
        let msgs = vec![user_msg("only turn"), assistant_msg("a")];
        assert_eq!(protect_from_index(&msgs), 0);
    }

    // ── microcompact_messages ───────────────────────────

    #[test]
    fn test_clears_old_exploratory_result() {
        let mut msgs = vec![
            user_msg("turn1"),                                      // 0
            tool_use_msg("t1", "Read"),                             // 1
            tool_result_msg("t1", "file content here — very long"), // 2 (old)
            assistant_msg("ok"),                                    // 3
            user_msg("turn2"),                                      // 4
            tool_use_msg("t2", "Read"),                             // 5
            tool_result_msg("t2", "recent content"),                // 6 (protected)
            assistant_msg("done"),                                  // 7
            user_msg("turn3"),                                      // 8
        ];
        let cleared = microcompact_messages(&mut msgs);
        assert_eq!(cleared, 1);

        // msg 2 (old Read) → 占位符
        if let ContentBlock::ToolResult { text, .. } = &msgs[2].content[0] {
            let t = text.as_ref().unwrap();
            assert!(t.contains("Old tool result cleared"), "got: {t}");
            assert!(t.contains("chars"), "got: {t}");
        } else {
            panic!("expected ToolResult");
        }

        // msg 6 (recent Read) → 原样
        if let ContentBlock::ToolResult { text, .. } = &msgs[6].content[0] {
            assert_eq!(text.as_ref().unwrap(), "recent content");
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn test_preserves_non_exploratory_result() {
        let mut msgs = vec![
            user_msg("turn1"),                         // 0
            tool_use_msg("e1", "Edit"),                // 1
            tool_result_msg("e1", "the diff content"), // 2 (old but non-exploratory)
            assistant_msg("ok"),                       // 3
            user_msg("turn2"),                         // 4
            tool_use_msg("r1", "Read"),                // 5
            tool_result_msg("r1", "file content"),     // 6 (protected)
            assistant_msg("done"),                     // 7
            user_msg("turn3"),                         // 8
        ];
        let cleared = microcompact_messages(&mut msgs);
        assert_eq!(cleared, 0); // Edit 不在白名单

        // msg 2 (old Edit) → 原样
        if let ContentBlock::ToolResult { text, .. } = &msgs[2].content[0] {
            assert_eq!(text.as_ref().unwrap(), "the diff content");
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn test_preserves_recent_turns() {
        let mut msgs = vec![
            user_msg("turn1"),                 // 0
            tool_use_msg("t1", "Read"),        // 1
            tool_result_msg("t1", "content1"), // 2 — in protect zone? No (protect_from=2)
            user_msg("turn2"),                 // 3
            tool_use_msg("t2", "Read"),        // 4
            tool_result_msg("t2", "content2"), // 5 — protected
            user_msg("turn3"),                 // 6
        ];
        // protect_from = index of 2nd-to-last User = index 3
        let cleared = microcompact_messages(&mut msgs);
        // msg 2 is at index 2, which is < protect_from(3), so NOT protected → cleared
        assert_eq!(cleared, 1);
    }

    #[test]
    fn test_no_text_field_uses_content() {
        let mut msgs = vec![
            user_msg("turn1"),                                           // 0
            tool_use_msg("t1", "Grep"),                                  // 1
            tool_result_msg_no_text("t1", "match line 1\nmatch line 2"), // 2 (old, no text)
            assistant_msg("ok"),                                         // 3
            user_msg("turn2"),                                           // 4
            assistant_msg("done"),                                       // 5
            user_msg("turn3"),                                           // 6
        ];
        let cleared = microcompact_messages(&mut msgs);
        assert_eq!(cleared, 1);
        // content 应被替换为占位符
        if let ContentBlock::ToolResult { content, text, .. } = &msgs[2].content[0] {
            let s = content.as_str().unwrap();
            assert!(s.contains("Old tool result cleared"));
            assert!(s.contains("25 chars")); // "match line 1\nmatch line 2" = 25 chars
                                             // text 也应被设置
            assert!(text.is_some());
        } else {
            panic!("expected ToolResult");
        }
    }

    #[test]
    fn test_too_few_messages_noop() {
        let mut msgs = vec![user_msg("hi"), assistant_msg("hello"), user_msg("bye")];
        let cleared = microcompact_messages(&mut msgs);
        assert_eq!(cleared, 0);
    }

    #[test]
    fn test_multiple_old_results_cleared() {
        let mut msgs = vec![
            user_msg("turn1"),                   // 0
            tool_use_msg("r1", "Read"),          // 1
            tool_result_msg("r1", "content r1"), // 2 (old)
            tool_use_msg("g1", "Grep"),          // 3
            tool_result_msg("g1", "match g1"),   // 4 (old)
            assistant_msg("ok"),                 // 5
            user_msg("turn2"),                   // 6
            assistant_msg("done"),               // 7
            user_msg("turn3"),                   // 8
        ];
        let cleared = microcompact_messages(&mut msgs);
        assert_eq!(cleared, 2);

        // 验证两个都被替换
        if let ContentBlock::ToolResult { text, .. } = &msgs[2].content[0] {
            assert!(text.as_ref().unwrap().contains("cleared"));
        }
        if let ContentBlock::ToolResult { text, .. } = &msgs[4].content[0] {
            assert!(text.as_ref().unwrap().contains("cleared"));
        }
    }

    // ── microcompact_messages 修复后的 turn 检测 ──────────

    #[test]
    fn test_tool_result_not_counted_as_turn_boundary() {
        // 修复前：ToolResult 是 Role::User，被误算为 turn → 保护边界偏移。
        // 修复后：is_real_user_turn 排除纯 ToolResult，保护边界正确。
        let mut msgs = vec![
            user_msg("turn1"),                 // 0 — real user turn
            tool_use_msg("t1", "Read"),        // 1
            tool_result_msg("t1", "content1"), // 2 — User but NOT a turn
            tool_use_msg("t2", "Read"),        // 3
            tool_result_msg("t2", "content2"), // 4 — User but NOT a turn
            assistant_msg("ok"),               // 5
            user_msg("turn2"),                 // 6 — real user turn
            assistant_msg("done"),             // 7
        ];
        // PROTECT_RECENT_TURNS=2 → protect_from = 倒数第 2 个 real user turn
        // 倒数第 2 个 real user turn = index 0 (turn1)
        // 所以 protect_from = 0 → 全部保护
        let cleared = microcompact_messages(&mut msgs);
        assert_eq!(cleared, 0);
    }

    // ── microcompact_chain ──────────────────────────────

    fn make_chain(num_segments: usize, tool_per_seg: usize) -> ChatChain {
        use crate::business::session::ChatSegment;
        let mut segments = Vec::new();
        let mut parent = None;
        for s in 0..num_segments {
            let mut seg = ChatSegment {
                id: format!("seg-{s}"),
                parent_id: parent.clone(),
                kind: crate::business::session::SegmentKind::Normal,
                summary: None,
                messages: vec![user_msg(&format!("turn{s}")), assistant_msg("reply")],
            };
            for t in 0..tool_per_seg {
                let id = format!("tu-{s}-{t}");
                seg.messages.push(tool_use_msg(&id, "Read"));
                seg.messages
                    .push(tool_result_msg(&id, &format!("file content {s}-{t}")));
            }
            parent = Some(seg.id.clone());
            segments.push(seg);
        }
        ChatChain::from_segments(segments)
    }

    #[test]
    fn test_microcompact_chain_protects_recent_3_segments() {
        // 5 segments, each with 1 Read result
        let mut chain = make_chain(5, 1);
        let cleared = microcompact_chain(&mut chain);
        // 最近 3 个保护，前 2 个折叠
        assert_eq!(cleared, 2);
    }

    #[test]
    fn test_microcompact_chain_noop_with_few_segments() {
        let mut chain = make_chain(3, 1);
        let cleared = microcompact_chain(&mut chain);
        // 仅 3 个 segment，全部保护
        assert_eq!(cleared, 0);
    }

    #[test]
    fn test_microcompact_chain_preserves_non_exploratory() {
        // segment 中只有 Edit 结果 → 不折叠
        use crate::business::session::{ChatChain, ChatSegment, SegmentKind};
        let seg1 = ChatSegment {
            id: "seg-0".to_string(),
            parent_id: None,
            kind: SegmentKind::Normal,
            summary: None,
            messages: vec![
                user_msg("turn0"),
                tool_use_msg("e1", "Edit"),
                tool_result_msg("e1", "diff content"),
                assistant_msg("ok"),
            ],
        };
        let mut parent = Some(seg1.id.clone());
        let mut segs = vec![seg1];
        for s in 1..5 {
            let seg = ChatSegment {
                id: format!("seg-{s}"),
                parent_id: parent.clone(),
                kind: SegmentKind::Normal,
                summary: None,
                messages: vec![user_msg(&format!("turn{s}")), assistant_msg("reply")],
            };
            parent = Some(seg.id.clone());
            segs.push(seg);
        }
        let mut chain = ChatChain::from_segments(segs);
        let cleared = microcompact_chain(&mut chain);
        // Edit 不在探索类白名单 → 0
        assert_eq!(cleared, 0);
    }
}

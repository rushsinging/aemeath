//! Microcompact：规则驱动清理陈旧探索类工具结果
//!
//! 在 auto-compact（LLM 摘要）之前、budget reduction 之后运行。
//! 纯规则判断，不调用 LLM，运行成本接近零。
//!
//! 策略：
//! 1. 扫描 messages，建立 tool_use_id → tool_name 映射。
//! 2. 以 User message 为分界，最近 `PROTECT_RECENT_TURNS` 轮内的 ToolResult 保护不动。
//! 3. 保护轮之外的探索类工具（Read/Grep/Glob/Bash/WebFetch/WebSearch 等）结果
//!    替换为占位符 `[Old tool result cleared (was N chars)]`。
//! 4. 非白名单工具（Edit/Write/TaskCreate 等）不受影响。

use share::message::{ContentBlock, Message, Role};
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

/// 保护最近 N 轮的 ToolResult 不被清理。
/// "轮"以 User message 为分界（连续两个 User message 之间为 1 轮）。
const PROTECT_RECENT_TURNS: usize = 2;

/// 占位符模板。
const PLACEHOLDER_TEMPLATE: &str = "[Old tool result cleared (was {n} chars)]";

/// 对 messages 执行 microcompact：原地替换陈旧探索类 tool result 为占位符。
///
/// 返回被清理的 ToolResult 数量。调用方可据此判断是否需要日志 / 事件通知。
/// 不改变 messages 的长度（不删除消息，只替换 content）。
pub fn microcompact_messages(messages: &mut [Message]) -> usize {
    if messages.len() <= 4 {
        return 0;
    }

    // 建立 tool_use_id → tool_name 映射
    let tool_names = build_tool_name_map(messages);

    // 计算保护边界：最近 PROTECT_RECENT_TURNS 轮的起始 message index
    let protect_from = protect_from_index(messages);

    let mut cleared = 0;
    for (i, msg) in messages.iter_mut().enumerate() {
        // protect_from 及之后的消息属于最近轮次，保护不动
        if i >= protect_from {
            break;
        }
        for block in msg.content.iter_mut() {
            if let ContentBlock::ToolResult {
                tool_use_id,
                content,
                text,
                ..
            } = block
            {
                // 通过 tool_use_id 查工具名
                let tool_name = tool_names.get(tool_use_id.as_str());
                if let Some(name) = tool_name {
                    if !is_exploratory(name) {
                        continue;
                    }
                } else {
                    // 未知工具名，保守保留
                    continue;
                }

                // 计算原始大小
                let original_size = tool_result_size(content, text.as_deref());
                if original_size == 0 {
                    continue;
                }

                // 替换为占位符
                let placeholder = PLACEHOLDER_TEMPLATE.replace("{n}", &original_size.to_string());
                *content = serde_json::Value::String(placeholder.clone());
                *text = Some(placeholder);
                cleared += 1;
            }
        }
    }

    cleared
}

/// 判断工具是否属于探索类（可被 microcompact 清理）。
fn is_exploratory(tool_name: &str) -> bool {
    EXPLORATORY_TOOLS.contains(&tool_name)
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

/// 计算保护边界的 message index：该 index 及之后的 ToolResult 不被清理。
///
/// 从末尾向前数 PROTECT_RECENT_TURNS 个 User message，最后一个 User message
/// 的 index 就是保护边界。该 User message 及之后的所有结果都保留。
fn protect_from_index(messages: &[Message]) -> usize {
    let mut user_count = 0;
    for (i, msg) in messages.iter().enumerate().rev() {
        if matches!(msg.role, Role::User) {
            user_count += 1;
            if user_count == PROTECT_RECENT_TURNS {
                return i;
            }
        }
    }
    // 不足 PROTECT_RECENT_TURNS 轮，保护全部
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
}

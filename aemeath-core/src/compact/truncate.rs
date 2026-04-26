//! 工具结果截断 — 单条结果和消息级别的长度限制
//!
//! 超长工具输出会在加入对话历史之前被截断为预览头 + 尾部。

use crate::message::{ContentBlock, Message};

/// 单条工具结果在截断前的最大字符数。
pub const MAX_TOOL_RESULT_CHARS: usize = 50_000;

/// 截断后保留的头部预览字符数。
pub const TRUNCATION_PREVIEW_HEAD: usize = 2_000;

/// 截断后保留的尾部字符数。
pub const TRUNCATION_PREVIEW_TAIL: usize = 500;

/// 单条消息中所有工具结果的最大总字符数。
/// 超过时优先截断最大的结果。
pub const MAX_TOOL_RESULTS_PER_MESSAGE_CHARS: usize = 200_000;

/// 对单条工具结果进行截断。如果未超过 `MAX_TOOL_RESULT_CHARS` 则原样返回。
pub fn truncate_tool_result(output: &str) -> String {
    if output.len() <= MAX_TOOL_RESULT_CHARS {
        return output.to_string();
    }

    let head = safe_slice(output, TRUNCATION_PREVIEW_HEAD);
    let tail = safe_slice_tail(output, TRUNCATION_PREVIEW_TAIL);

    format!(
        "{head}\n\n[... truncated {} chars, showing first {} + last {} chars ...]\n\n{tail}",
        output.len(),
        head.len(),
        tail.len(),
    )
}

/// 对单条消息施加工具结果总长度预算。
/// 如果总字符数超过限制，优先截断最大的结果。
pub fn apply_tool_result_budget(message: &mut Message) {
    let mut result_sizes: Vec<(usize, usize)> = Vec::new(); // (index, size)
    let mut total_chars = 0usize;

    for (i, block) in message.content.iter().enumerate() {
        if let ContentBlock::ToolResult { content, .. } = block {
            let size = match content {
                serde_json::Value::String(s) => s.len(),
                _ => content.to_string().len(),
            };
            result_sizes.push((i, size));
            total_chars += size;
        }
    }

    if total_chars <= MAX_TOOL_RESULTS_PER_MESSAGE_CHARS {
        return;
    }

    // 按大小降序排列 — 优先截断最大的
    result_sizes.sort_by(|a, b| b.1.cmp(&a.1));

    for (idx, _size) in result_sizes {
        if total_chars <= MAX_TOOL_RESULTS_PER_MESSAGE_CHARS {
            break;
        }
        if let ContentBlock::ToolResult { ref mut content, .. } = message.content[idx] {
            let old_text = match content {
                serde_json::Value::String(s) => s.clone(),
                _ => content.to_string(),
            };
            if old_text.len() > MAX_TOOL_RESULT_CHARS {
                let truncated = truncate_tool_result(&old_text);
                total_chars -= old_text.len();
                total_chars += truncated.len();
                *content = serde_json::Value::String(truncated);
            }
        }
    }
}

/// 对 (id, output, is_error, images) 元组列表中的工具结果进行截断。
pub fn truncate_tool_results(
    results: &mut Vec<(String, String, bool, Vec<crate::tool::ImageData>)>,
) {
    for (_id, output, _is_error, _images) in results.iter_mut() {
        if output.len() > MAX_TOOL_RESULT_CHARS {
            *output = truncate_tool_result(output);
        }
    }
}

/// 从字符串开头安全切片，确保不拆分 UTF-8 字符边界。
pub fn safe_slice(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// 从字符串末尾安全切片。
pub fn safe_slice_tail(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut start = s.len() - max_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{ContentBlock, Message};

    // ── safe_slice ──────────────────────────────────────────────

    #[test]
    fn safe_slice_short_string_unchanged() {
        assert_eq!(safe_slice("hello", 10), "hello");
    }

    #[test]
    fn safe_slice_ascii_exact_truncation() {
        assert_eq!(safe_slice("hello world", 5), "hello");
    }

    #[test]
    fn safe_slice_multibyte_no_char_split() {
        // "你好世界" — each char is 3 bytes in UTF-8
        let s = "你好世界";
        // 4 bytes falls inside the second Chinese character (bytes 3-5)
        // should back up to byte 3, returning "你"
        assert_eq!(safe_slice(s, 4), "你");
    }

    #[test]
    fn safe_slice_at_char_boundary_returns_directly() {
        let s = "abc你好";
        // byte 3 is exactly the boundary before '你'
        assert_eq!(safe_slice(s, 3), "abc");
    }

    // ── safe_slice_tail ─────────────────────────────────────────

    #[test]
    fn safe_slice_tail_short_string_unchanged() {
        assert_eq!(safe_slice_tail("hello", 10), "hello");
    }

    #[test]
    fn safe_slice_tail_ascii_truncation() {
        assert_eq!(safe_slice_tail("hello world", 5), "world");
    }

    #[test]
    fn safe_slice_tail_multibyte_no_char_split() {
        let s = "你好世界";
        // 4 bytes from the end: bytes 7-11 ("界" is bytes 9-11)
        // should skip forward to char boundary at byte 9, returning "界"
        assert_eq!(safe_slice_tail(s, 4), "界");
    }

    // ── truncate_tool_result ────────────────────────────────────

    #[test]
    fn truncate_tool_result_short_text_unchanged() {
        let short = "a".repeat(100);
        assert_eq!(truncate_tool_result(&short), short);
    }

    #[test]
    fn truncate_tool_result_long_text_truncated() {
        let long = "a".repeat(MAX_TOOL_RESULT_CHARS + 10);
        let result = truncate_tool_result(&long);
        assert!(result.contains("[... truncated"));
        // should contain head portion
        assert!(result.starts_with(&"a".repeat(TRUNCATION_PREVIEW_HEAD)));
        // should contain tail portion
        assert!(result.ends_with(&"a".repeat(TRUNCATION_PREVIEW_TAIL)));
    }

    // ── apply_tool_result_budget ────────────────────────────────

    #[test]
    fn apply_tool_result_budget_under_limit_unchanged() {
        let mut msg = Message {
            role: crate::message::Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: serde_json::Value::String("short".to_string()),
                is_error: false,
            }],
        };
        apply_tool_result_budget(&mut msg);
        match &msg.content[0] {
            ContentBlock::ToolResult { content, .. } => {
                assert_eq!(content, &serde_json::Value::String("short".to_string()));
            }
            _ => panic!("expected ToolResult"),
        }
    }

    #[test]
    fn apply_tool_result_budget_over_limit_truncates_largest() {
        let large_content = "x".repeat(MAX_TOOL_RESULTS_PER_MESSAGE_CHARS + 1000);
        let mut msg = Message {
            role: crate::message::Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: serde_json::Value::String(large_content.clone()),
                is_error: false,
            }],
        };
        apply_tool_result_budget(&mut msg);
        match &msg.content[0] {
            ContentBlock::ToolResult { content, .. } => {
                let text = content.as_str().unwrap();
                // Should have been truncated — significantly shorter than original
                assert!(text.len() < large_content.len());
                assert!(text.contains("[... truncated"));
            }
            _ => panic!("expected ToolResult"),
        }
    }
}

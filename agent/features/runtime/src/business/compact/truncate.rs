//! 工具结果截断 — 单条结果和消息级别的长度限制
//!
//! 超长工具输出会在加入对话历史之前被截断为预览头 + 尾部。
//! 落盘逻辑在 `storage::tool_result_storage` 中实现（`persist_oversized_results`），
//! 本模块仅提供截断兜底（当落盘失败或未被调用时的纯截断）。

use share::message::{ContentBlock, Message};
use share::string_idx::{slice_head, slice_tail};

/// 截断后保留的头部预览字符数。
pub const TRUNCATION_PREVIEW_HEAD: usize = 2_000;

/// 截断后保留的尾部字符数。
pub const TRUNCATION_PREVIEW_TAIL: usize = 500;

/// 单条消息中所有工具结果的最大总字符数。
/// 超过时优先截断最大的结果。
pub const MAX_TOOL_RESULTS_PER_MESSAGE_CHARS: usize = 200_000;

/// 对单条工具结果进行截断。如果未超过 `storage::MAX_TOOL_RESULT_CHARS` 则原样返回。
pub fn truncate_tool_result(output: &str) -> String {
    if output.len() <= storage::api::MAX_TOOL_RESULT_CHARS {
        return output.to_string();
    }

    let head = slice_head(output, TRUNCATION_PREVIEW_HEAD);
    let tail = slice_tail(output, TRUNCATION_PREVIEW_TAIL);

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
    result_sizes.sort_by_key(|item| std::cmp::Reverse(item.1));

    for (idx, _size) in result_sizes {
        if total_chars <= MAX_TOOL_RESULTS_PER_MESSAGE_CHARS {
            break;
        }
        if let ContentBlock::ToolResult {
            ref mut content, ..
        } = message.content[idx]
        {
            let old_text = match content {
                serde_json::Value::String(s) => s.clone(),
                _ => content.to_string(),
            };
            if old_text.len() > storage::api::MAX_TOOL_RESULT_CHARS {
                let truncated = truncate_tool_result(&old_text);
                total_chars -= old_text.len();
                total_chars += truncated.len();
                *content = serde_json::Value::String(truncated);
            }
        }
    }
}

/// 对 (id, output, is_error, images) 元组列表中的工具结果进行截断。
pub fn truncate_tool_results(results: &mut [(String, String, bool, Vec<share::tool::ImageData>)]) {
    for (_id, output, _is_error, _images) in results.iter_mut() {
        if output.len() > storage::api::MAX_TOOL_RESULT_CHARS {
            *output = truncate_tool_result(output);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::message::{ContentBlock, Message};

    // ── truncate_tool_result ────────────────────────────────────

    #[test]
    fn truncate_tool_result_short_text_unchanged() {
        let short = "a".repeat(100);
        assert_eq!(truncate_tool_result(&short), short);
    }

    #[test]
    fn truncate_tool_result_long_text_truncated() {
        let long = "a".repeat(storage::api::MAX_TOOL_RESULT_CHARS + 10);
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
            role: share::message::Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: serde_json::Value::String("short".to_string()),
                is_error: false,
                text: None,
            }],
            metadata: None,
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
            role: share::message::Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: serde_json::Value::String(large_content.clone()),
                is_error: false,
                text: None,
            }],
            metadata: None,
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

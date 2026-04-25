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

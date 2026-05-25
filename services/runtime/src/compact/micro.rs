//! 微压缩 (Microcompact)
//!
//! 清除旧消息中的工具结果内容以节省 token，仅保留最近消息的完整输出。

use aemeath_core::message::{ContentBlock, Message};

/// 微压缩：清除旧工具结果以节省 token。
/// 仅保留最近 `keep_recent` 条消息的工具结果内容不变。
pub fn microcompact(messages: &mut Vec<Message>, keep_recent: usize) {
    if messages.len() <= keep_recent {
        return;
    }

    let cutoff = messages.len() - keep_recent;
    for msg in messages[..cutoff].iter_mut() {
        for block in msg.content.iter_mut() {
            if let ContentBlock::ToolResult {
                ref mut content, ..
            } = block
            {
                let content_len = match content {
                    serde_json::Value::String(s) => s.len(),
                    _ => content.to_string().len(),
                };
                if content_len > 100 {
                    *content = serde_json::Value::String(format!(
                        "[output truncated, was {} chars]",
                        content_len
                    ));
                }
            }
        }
    }
}

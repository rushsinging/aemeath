//! tool_result 图片块提取（#1992）。
//!
//! canonical 的 `ToolResult.content` 在 `to_llm_view` 后可能是
//! `[image, text]` 数组（Read 读图等）。Anthropic 协议原生支持
//! tool_result 内嵌 image；Chat Completions / Responses / Ollama 的
//! tool 消息协议均不承载图片，出站转换 MUST 把 image 块提升为紧随的
//! user 消息，否则图片被静默丢弃（模型只看到文本元信息）。

use serde_json::Value;

/// 从 tool_result 的 `content` 数组提取 `(media_type, base64_data)` 列表。
///
/// 非数组（纯文本结果）或形状不合法的块返回空列表；不报错、不 panic。
pub(crate) fn extract_tool_result_images(content: &Value) -> Vec<(String, String)> {
    let Value::Array(blocks) = content else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(|t| t.as_str()) == Some("image"))
        .filter_map(|block| {
            let source = block.get("source")?;
            let media_type = source.get("media_type")?.as_str()?.to_string();
            let data = source.get("data")?.as_str()?.to_string();
            Some((media_type, data))
        })
        .collect()
}

/// 提取该 ToolResult 文本以外的内容：`content` 数组中的 text 块拼接。
///
/// tool 消息只能承载文本；数组形态下 text 块需手动拼接（既有行为收敛至此）。
pub(crate) fn extract_tool_result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join(""),
        other => other.to_string(),
    }
}

#[cfg(test)]
#[path = "tool_result_images_tests.rs"]
mod tests;

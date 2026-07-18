//! 消息构造方法

use crate::message::types::*;
use serde_json;

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            metadata: None,
        }
    }

    pub fn system_generated_user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            metadata: Some(MessageMetadata {
                source: MessageSource::SystemGenerated,
            }),
        }
    }

    pub fn user_with_image(
        text: impl Into<String>,
        image_base64: String,
        media_type: String,
    ) -> Self {
        Self {
            role: Role::User,
            content: vec![
                ContentBlock::base64_image(image_base64, media_type),
                ContentBlock::Text { text: text.into() },
            ],
            metadata: None,
        }
    }

    /// 按 `text` 中 `[Image #N]` 占位符与 `images` 顺序穿插拆 block（#fix-tui-image-input-output）。
    ///
    /// - `images[i]` 的 `id` 为 `[Image #i+1]`（**0-based ↔ 1-based**），与 `text` 中占位符一一对应
    /// - 出现顺序按 `text` 扫到的 `[Image #1]`、`[Image #2]`、... 穿插插入 image block
    /// - **text 中无任何占位符**时，**保持原头尾行为**（Image 块在前、Text 块在后），
    ///   兼容旧 session 持久化数据（#fix-tui-image-input-output 后向兼容）
    /// - 单图且 text 无占位 → `[Image] + text`（向后兼容旧 session）
    ///
    /// **不发给 LLM 时** provider adapter 拿 `Vec<ContentBlock>` 已经正确交替，
    /// 无需再做拆分；`placeholder` 字段 round-trip 用，adapter 端丢弃。
    pub fn user_with_images(
        text: impl Into<String>,
        images: Vec<(String, String, String)>,
    ) -> Self {
        let text = text.into();
        if images.is_empty() {
            return Self::user(text);
        }
        // 检查 text 中是否含至少一个占位符；没有则走旧头尾路径
        let has_placeholder = images.iter().any(|(ph, _, _)| text.contains(ph.as_str()));
        if !has_placeholder {
            // 向后兼容：[Image, Image, ..., Text]
            let mut content: Vec<ContentBlock> = images
                .into_iter()
                .map(|(placeholder, data, media_type)| ContentBlock::Image {
                    source: ImageSource::Base64 { media_type, data },
                    placeholder: Some(placeholder),
                })
                .collect();
            content.push(ContentBlock::Text { text });
            return Self {
                role: Role::User,
                content,
                metadata: None,
            };
        }

        // images 按占位符 id 升序排（[Image #1] → [Image #2] → ...），便于文本扫描时定位
        let mut sorted_images = images;
        sorted_images.sort_by(|a, b| a.0.cmp(&b.0));

        let mut content: Vec<ContentBlock> = Vec::new();
        let mut cursor = 0usize;
        let mut used = vec![false; sorted_images.len()];
        while cursor <= text.len() {
            // 找下一个最近、未使用的 [Image #N] 占位符
            let mut next_pos: Option<(usize, usize, usize)> = None; // (byte_pos, end, image_idx)
            for (idx, img) in sorted_images.iter().enumerate() {
                if used[idx] {
                    continue;
                }
                if let Some(pos) = text[cursor..].find(&img.0) {
                    let abs_pos = cursor + pos;
                    if next_pos.is_none_or(|(p, _, _)| abs_pos < p) {
                        next_pos = Some((abs_pos, abs_pos + img.0.len(), idx));
                    }
                }
            }
            match next_pos {
                Some((start, end, idx)) => {
                    if start > cursor {
                        content.push(ContentBlock::Text {
                            text: text[cursor..start].to_string(),
                        });
                    }
                    let (placeholder, data, media_type) = sorted_images[idx].clone();
                    content.push(ContentBlock::Image {
                        source: ImageSource::Base64 { media_type, data },
                        placeholder: Some(placeholder),
                    });
                    used[idx] = true;
                    cursor = end;
                }
                None => break,
            }
        }
        if cursor < text.len() {
            content.push(ContentBlock::Text {
                text: text[cursor..].to_string(),
            });
        }
        // 未配对成功的 image 全堆尾部
        for (idx, (placeholder, data, media_type)) in sorted_images.into_iter().enumerate() {
            if !used[idx] {
                content.push(ContentBlock::Image {
                    source: ImageSource::Base64 { media_type, data },
                    placeholder: Some(placeholder),
                });
            }
        }
        if content.is_empty() {
            content.push(ContentBlock::Text { text });
        }
        Self {
            role: Role::User,
            content,
            metadata: None,
        }
    }

    pub fn tool_results(results: Vec<(String, String, bool)>) -> Self {
        Self {
            role: Role::User,
            content: results
                .into_iter()
                .map(
                    |(tool_use_id, content, is_error)| ContentBlock::ToolResult {
                        tool_use_id,
                        content: serde_json::Value::String(content),
                        is_error,
                        // content 已是纯文本，本身即 text-first；无需单独 text。
                        text: None,
                    },
                )
                .collect(),
            metadata: None,
        }
    }

    /// Create tool results with optional image attachments.
    /// Each result is (tool_use_id, text_content, json_content, is_error, images).
    pub fn tool_results_rich<I>(
        results: Vec<(String, String, serde_json::Value, bool, Vec<I>)>,
    ) -> Self
    where
        I: Into<(String, String)>,
    {
        Self {
            role: Role::User,
            content: results
                .into_iter()
                .map(|(tool_use_id, text, json_content, is_error, images)| {
                    let content = if images.is_empty() {
                        json_content
                    } else {
                        let mut blocks: Vec<serde_json::Value> = images
                            .into_iter()
                            .map(|img| {
                                let (base64, media_type) = img.into();
                                serde_json::json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": media_type,
                                        "data": base64,
                                    }
                                })
                            })
                            .collect();
                        blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text.clone(),
                        }));
                        blocks.push(serde_json::json!({
                            "type": "json",
                            "json": json_content,
                        }));
                        serde_json::Value::Array(blocks)
                    };
                    // 持久化忠实保留结构化 content；text 供 LLM 出站时降为 text-first。
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                        text: Some(text),
                    }
                })
                .collect(),
            metadata: None,
        }
    }
}

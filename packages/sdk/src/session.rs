//! Session 快照与摘要。

use crate::content::ContentBlock;
use crate::InputId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// SDK 级 message 投影。`content` 为 typed 块列表（serde 成与历史完全相同的 JSON）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChatMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ChatMessageMetadata>,
    /// TUI 输入归宿（#507 修复）：runtime→TUI 边界标识"哪次用户输入产生了这条 Message"，
    /// TUI 据此按 id 清对应占位。session 持久化默认不带此字段（`skip_serializing_if`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<InputId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChatMessageMetadata {
    #[serde(default)]
    pub source: ChatMessageSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_hook: Option<StopHookFeedbackView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StopHookFeedbackView {
    pub summary: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub reason: String,
    pub stdout_preview: String,
    pub stderr_preview: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_file: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChatMessageSource {
    #[default]
    User,
    SystemGenerated,
    StopHook,
}

impl ChatMessage {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![ContentBlock::text(text)],
            metadata: None,
            input_id: None,
        }
    }

    pub fn system_generated_user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![ContentBlock::text(text)],
            metadata: Some(ChatMessageMetadata {
                source: ChatMessageSource::SystemGenerated,
                stop_hook: None,
            }),
            input_id: None,
        }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: vec![ContentBlock::text(text)],
            metadata: None,
            input_id: None,
        }
    }

    /// 按 `text` 中 `[Image #N]` 占位符与 `images` 顺序穿插拆 block（#fix-tui-image-input-output）。
    ///
    /// - `images[i]` 的 `id` 为 `[Image #i+1]`（**0-based ↔ 1-based**），与 `text` 中占位符一一对应
    /// - 出现顺序按 `text` 扫到的 `[Image #1]`、`[Image #2]`、... 穿插插入 image block
    /// - **text 中无任何占位符**时，**保持原头尾行为**（Image 块在前、Text 块在后），
    ///   兼容旧 session 持久化数据（#fix-tui-image-input-output 后向兼容）
    ///
    /// **不发给 LLM 时** provider adapter 拿 `Vec<ContentBlock>` 已经正确交替，
    /// 无需再做拆分；`placeholder` 字段 round-trip 用，adapter 端丢弃。
    pub fn user_with_images(text: impl Into<String>, images: Vec<crate::ChatInputImage>) -> Self {
        let text = text.into();
        if images.is_empty() {
            return Self {
                role: "user".to_string(),
                content: vec![ContentBlock::text(text)],
                metadata: None,
                input_id: None,
            };
        }
        // 检查 text 中是否含至少一个占位符；没有则走头尾路径
        let has_placeholder = images.iter().any(|img| text.contains(&img.id));
        if !has_placeholder {
            // 头尾：[Image, Image, ..., Text]
            let mut content: Vec<ContentBlock> = images
                .into_iter()
                .map(|image| ContentBlock::Image {
                    source: crate::content::ImageSource::Base64 {
                        media_type: image.media_type,
                        data: image.base64,
                    },
                    placeholder: Some(image.id),
                })
                .collect();
            content.push(ContentBlock::text(text));
            return Self {
                role: "user".to_string(),
                content,
                metadata: None,
                input_id: None,
            };
        }
        // 按占位符 id 升序排，便于文本扫描时定位
        let mut sorted_images = images;
        sorted_images.sort_by(|a, b| a.id.cmp(&b.id));

        let mut content: Vec<ContentBlock> = Vec::new();
        let mut cursor = 0usize;
        let mut used = vec![false; sorted_images.len()];
        while cursor <= text.len() {
            let mut next_pos: Option<(usize, usize, usize)> = None; // (byte_pos, end, image_idx)
            for (idx, img) in sorted_images.iter().enumerate() {
                if used[idx] {
                    continue;
                }
                if let Some(pos) = text[cursor..].find(&img.id) {
                    let abs_pos = cursor + pos;
                    if next_pos.is_none_or(|(p, _, _)| abs_pos < p) {
                        next_pos = Some((abs_pos, abs_pos + img.id.len(), idx));
                    }
                }
            }
            match next_pos {
                Some((start, end, idx)) => {
                    if start > cursor {
                        content.push(ContentBlock::text(&text[cursor..start]));
                    }
                    let image = &sorted_images[idx];
                    content.push(ContentBlock::Image {
                        source: crate::content::ImageSource::Base64 {
                            media_type: image.media_type.clone(),
                            data: image.base64.clone(),
                        },
                        placeholder: Some(image.id.clone()),
                    });
                    used[idx] = true;
                    cursor = end;
                }
                None => break,
            }
        }
        if cursor < text.len() {
            content.push(ContentBlock::text(&text[cursor..]));
        }
        // 未配对成功的 image 全堆尾部
        for (idx, image) in sorted_images.iter().enumerate() {
            if !used[idx] {
                content.push(ContentBlock::Image {
                    source: crate::content::ImageSource::Base64 {
                        media_type: image.media_type.clone(),
                        data: image.base64.clone(),
                    },
                    placeholder: Some(image.id.clone()),
                });
            }
        }
        if content.is_empty() {
            content.push(ContentBlock::text(text));
        }
        Self {
            role: "user".to_string(),
            content,
            metadata: None,
            input_id: None,
        }
    }

    /// 是否为**用户输入**消息：role=user + source=User + 含 Text 块。
    /// 显式分类，取代历史 `text_content().is_empty()` 启发式（修 #386 那类）。
    pub fn is_user_input(&self) -> bool {
        self.role == "user"
            && self.source() == ChatMessageSource::User
            && self.content.iter().any(|b| b.is_text())
    }

    /// 是否含工具结果块。
    pub fn has_tool_result(&self) -> bool {
        self.content.iter().any(|b| b.is_tool_result())
    }

    /// 消息文本：拼接 Text 块 + Image.placeholder（`[Image #N]`）。
    ///
    /// #507 修复：原实现只取 Text 块，丢 Image.placeholder → TUI 回显丢失占位符。
    /// 现对齐 share::Message::text_content() 的拼接逻辑，按 block 顺序还原
    /// "Text + Image.placeholder + Text" 完整文本。ToolResult/Thinking 等其它块不参与。
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => text.as_str(),
                ContentBlock::Image {
                    placeholder: Some(ph),
                    ..
                } => ph.as_str(),
                _ => "",
            })
            .collect()
    }

    pub fn source(&self) -> ChatMessageSource {
        self.metadata
            .as_ref()
            .map(|metadata| metadata.source)
            .unwrap_or_default()
    }
}

/// Session 快照（cheap clone）。
///
/// 底层 Vec 消息通过 Arc 共享，clone 开销低。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionSnapshot {
    /// Session ID。
    pub id: String,
    /// 消息列表摘要（消息数量）。
    pub message_count: usize,
    /// 总 token 使用量。
    pub total_tokens: u64,
    /// 完整消息列表（仅在 load_session 时填充，snapshot 为 None）。
    pub messages: Vec<ChatMessage>,
    /// 创建时间（ISO 8601）。
    pub created_at: Option<String>,
    /// 消息清洗中移除的消息数。
    pub trimmed: usize,
    /// 消息清洗中修复的消息数。
    pub repaired: usize,
    /// 会话 workspace 上下文（若存在）。
    pub workspace: Option<crate::WorkspaceContextView>,
    /// 任务快照（Session 恢复时用于重建任务状态）。
    pub tasks: Option<serde_json::Value>,
}

/// Session 列表中的摘要条目。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionSummary {
    /// Session ID。
    pub id: String,
    /// 用户自定义标题。
    pub title: Option<String>,
    /// 所属项目。
    pub project: Option<String>,
    /// 使用模型。
    pub model: Option<String>,
    /// 创建时间。
    pub created_at: String,
    /// 最后更新时间。
    pub updated_at: String,
    /// 消息数量。
    pub message_count: usize,
    /// 首条用户消息预览。
    pub preview: Option<String>,
    /// 展示摘要。
    pub summary: String,
}

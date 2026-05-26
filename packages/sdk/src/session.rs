//! Session 快照与摘要。

use serde::{Deserialize, Serialize};

/// Session 快照（cheap clone）。
///
/// 底层 Vec 消息通过 Arc 共享，clone 开销低。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Session ID。
    pub id: String,
    /// 消息列表摘要（消息数量）。
    pub message_count: usize,
    /// 总 token 使用量。
    pub total_tokens: u64,
}

/// Session 列表中的摘要条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    /// Session ID。
    pub id: String,
    /// 创建时间。
    pub created_at: String,
    /// 最后更新时间。
    pub updated_at: String,
    /// 消息数量。
    pub message_count: usize,
    /// 首条用户消息预览。
    pub preview: Option<String>,
}

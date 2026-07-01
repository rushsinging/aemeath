//! ConversationUpdate trait：intent 自治分发机制。
//!
//! 每个 intent 是独立 struct，自带 `update()` 逻辑。`ConversationIntent` enum
//! 保留为传输容器，只做 match 转发。

use super::change::ConversationChange;
use super::model::ConversationModel;

/// 所有 intent struct 实现此 trait，将自身逻辑应用于 model。
pub trait ConversationUpdate {
    fn update(self, model: &mut ConversationModel) -> Vec<ConversationChange>;
}

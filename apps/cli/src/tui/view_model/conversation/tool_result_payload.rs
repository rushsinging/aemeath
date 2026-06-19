use serde_json::Value;
use std::hash::{Hash, Hasher};

/// TUI 渲染层 view_model 视角的 tool result owned payload。
///
/// 与 model 端的 ToolResultPayload 字段一致；
/// 在 view_model 层重复定义避免 view_model → model 的跨层依赖（架构规则
/// `check-tui-model-view-boundaries.sh` 禁止 view_model import model 任何内容）。
///
/// view_assembler/output.rs 负责在组装 ToolCallBlockView 时把 model 端 payload
/// 字段（output / content / is_error / image_count）转成本类型，确保 view_model
/// 与 model 解耦。
#[derive(Clone, Debug, PartialEq)]
pub struct ToolResultPayload {
    pub output: String,
    pub content: Value,
    pub is_error: bool,
    pub image_count: usize,
}

impl ToolResultPayload {
    pub fn new(output: String, content: Value, is_error: bool, image_count: usize) -> Self {
        Self {
            output,
            content,
            is_error,
            image_count,
        }
    }
}

// 手写 `Eq` 与 `Hash`：serde_json::Value 不 impl Eq/Hash，但我们的缓存键只需要
// `output`/`is_error`/`image_count` 三个标识字段的指纹——`content` 走 partial_eq
// 比较（derive PartialEq 已用），对 cache_version 的语义指纹无影响。
impl Eq for ToolResultPayload {}

impl Hash for ToolResultPayload {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.output.hash(state);
        self.is_error.hash(state);
        self.image_count.hash(state);
    }
}

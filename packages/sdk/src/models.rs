//! 模型列表投影。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `aemeath models` 展示所需的模型摘要。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct ModelSummary {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub context_window: usize,
    pub max_tokens: u32,
}

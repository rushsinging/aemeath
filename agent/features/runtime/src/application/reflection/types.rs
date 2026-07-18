use serde::{Deserialize, Deserializer, Serialize};

/// 将 `null` 视为空 Vec 的兼容反序列化器。
///
/// LLM 在无内容时可能返回 `"field": null`，而 `#[serde(default)]` 只在字段
/// **缺失**时生效，显式 `null` 仍会触发反序列化错误。
fn null_as_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let opt = Option::<Vec<T>>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySuggestion {
    #[serde(default = "default_memory_layer")]
    pub layer: share::memory::MemoryLayer,
    pub category: share::memory::MemoryCategory,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub reason: String,
}

fn default_memory_layer() -> share::memory::MemoryLayer {
    share::memory::MemoryLayer::Project
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionOutput {
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub deviations: Vec<String>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub suggested_memories: Vec<MemorySuggestion>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub outdated_memories: Vec<String>,
    #[serde(default)]
    pub user_alert: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReflectionApplyResult {
    pub suggestions_added: usize,
    pub outdated_marked: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ReflectionError {
    #[error("failed to parse reflection JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error(transparent)]
    Memory(#[from] memory::MemoryError),
    #[error("failed to apply reflection memory suggestion: {0}")]
    Apply(String),
    #[error("reflection memory store initialization failed: {0}")]
    StoreInit(String),
    #[error("reflection LLM call failed: {0}")]
    LlmCall(String),
    #[error("reflection LLM returned empty response")]
    EmptyResponse,
    #[error("reflection LLM response could not be parsed as JSON (first 200 chars): {0}")]
    Unparseable(String),
}

pub type ReflectionResult<T> = Result<T, ReflectionError>;

pub struct ReflectionEngine;

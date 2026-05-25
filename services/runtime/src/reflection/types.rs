use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySuggestion {
    pub category: aemeath_core::memory::MemoryCategory,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionOutput {
    #[serde(default)]
    pub deviations: Vec<String>,
    #[serde(default)]
    pub suggested_memories: Vec<MemorySuggestion>,
    #[serde(default)]
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
    Memory(#[from] aemeath_core::memory::MemoryError),
}

pub type ReflectionResult<T> = Result<T, ReflectionError>;

pub struct ReflectionEngine;

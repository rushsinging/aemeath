//! ModelEntryConfig 自定义反序列化
//!
//! 支持 reasoning 字段的灵活格式：
//! - `"reasoning": true/false` → `Option<bool>` as-is
//! - `"reasoning": { "effort": "medium" }` → `reasoning: Some(true)`, `reasoning_effort: Some("medium")`

use crate::config::models::types::ModelEntryConfig;
use serde::{Deserialize, Deserializer};

impl<'de> Deserialize<'de> for ModelEntryConfig {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            id: String,
            #[serde(default)]
            name: String,
            #[serde(default)]
            input: Vec<String>,
            #[serde(default, rename = "contextWindow")]
            context_window: usize,
            #[serde(default, rename = "max_tokens", alias = "maxTokens")]
            max_tokens: u32,
            #[serde(default, rename = "thinking_max_tokens", alias = "thinkingMaxTokens")]
            thinking_max_tokens: u32,
            #[serde(default)]
            reasoning: FlexReasoning,
            #[serde(default)]
            reasoning_effort: Option<String>,
        }

        /// Flexible reasoning: accepts bool or { "effort": "..." } object.
        #[derive(Deserialize, Default)]
        #[serde(untagged)]
        enum FlexReasoning {
            #[default]
            None,
            Bool(bool),
            Effort {
                effort: String,
            },
        }

        let raw = Raw::deserialize(de)?;
        let (reasoning, reasoning_effort) = match raw.reasoning {
            FlexReasoning::None => (None, raw.reasoning_effort),
            FlexReasoning::Bool(b) => (Some(b), raw.reasoning_effort),
            FlexReasoning::Effort { effort } => {
                // Object form implies reasoning is enabled; effort merges
                // (field-level value wins over object-level)
                (Some(true), Some(raw.reasoning_effort.unwrap_or(effort)))
            }
        };

        Ok(ModelEntryConfig {
            id: raw.id,
            name: raw.name,
            input: raw.input,
            context_window: raw.context_window,
            max_tokens: raw.max_tokens,
            thinking_max_tokens: raw.thinking_max_tokens,
            reasoning,
            reasoning_effort,
        })
    }
}

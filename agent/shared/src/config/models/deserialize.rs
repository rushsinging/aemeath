//! ModelEntryConfig 自定义反序列化
//!
//! 仅接受 `reasoning: Option<bool>`，旧格式 { "effort": "..." } 不再支持。

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
            #[serde(default)]
            reasoning: Option<bool>,
        }

        let raw = Raw::deserialize(de)?;
        Ok(ModelEntryConfig {
            id: raw.id,
            name: raw.name,
            input: raw.input,
            context_window: raw.context_window,
            max_tokens: raw.max_tokens,
            reasoning: raw.reasoning,
        })
    }
}

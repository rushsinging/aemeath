//! ModelEntryConfig 自定义反序列化
//!
//! 接受 `reasoning: Option<bool>` 与 `reasoning_effort: Option<String>`；
//! 旧格式 { "effort": "..." } 不再支持。

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
            #[serde(default)]
            reasoning_effort: Option<String>,
        }

        let raw = Raw::deserialize(de)?;
        Ok(ModelEntryConfig {
            id: raw.id,
            name: raw.name,
            input: raw.input,
            context_window: raw.context_window,
            max_tokens: raw.max_tokens,
            reasoning: raw.reasoning,
            reasoning_effort: raw.reasoning_effort,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::config::models::types::ModelEntryConfig;

    #[test]
    fn test_deserialize_reads_reasoning_effort() {
        let json = r#"{ "id": "m", "reasoning": true, "reasoning_effort": "xhigh" }"#;
        let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(entry.reasoning, Some(true));
        assert_eq!(entry.reasoning_effort.as_deref(), Some("xhigh"));
    }

    #[test]
    fn test_deserialize_reasoning_effort_defaults_none() {
        let json = r#"{ "id": "m", "reasoning": true }"#;
        let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(entry.reasoning_effort, None);
    }
}

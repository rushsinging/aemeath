//! Chat Provider driver 抽象：不同供应商的推理字段差异化处理

use crate::api::ProviderDriverKind;
use crate::core::provider::ReasoningLevel;

use super::ReasoningConfig;

/// 将 thinking tokens 数量映射到 effort 级别
pub fn effort_from_thinking_tokens(tokens: u32) -> &'static str {
    match tokens {
        0..=1024 => "low",
        1025..=8192 => "medium",
        8193..=32768 => "high",
        _ => "xhigh",
    }
}

pub trait ChatApiDriver: Send + Sync {
    fn max_tokens_field(&self) -> &'static str {
        "max_tokens"
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    );

    /// 此 driver 支持的最高 ReasoningLevel。
    /// 默认 High，各 driver 按能力覆盖。
    fn max_reasoning_level(&self) -> ReasoningLevel {
        ReasoningLevel::High
    }
}

#[derive(Debug)]
pub struct OpenAiDriver;

#[derive(Debug)]
pub struct ZhipuDriver;

#[derive(Debug)]
pub struct LiteLlmDriver;

#[derive(Debug)]
pub struct VolcengineDriver;

#[derive(Debug)]
pub struct MinimaxDriver;

#[derive(Debug)]
pub struct MimoDriver;

impl ChatApiDriver for OpenAiDriver {
    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        _reasoning_enabled: bool,
    ) {
        if let Some(ReasoningConfig::Object(reasoning)) = reasoning_config {
            request_body["reasoning"] = reasoning.clone();
        } else if let Some(ReasoningConfig::ThinkingBudget(tokens)) = reasoning_config {
            request_body["reasoning"] =
                serde_json::json!({"effort": effort_from_thinking_tokens(*tokens)});
        }
    }
}

impl ChatApiDriver for ZhipuDriver {
    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    ) {
        let enabled = match reasoning_config {
            Some(ReasoningConfig::Bool(value)) => *value,
            _ => reasoning_enabled,
        };
        let thinking_type = if enabled { "enabled" } else { "disabled" };
        request_body["thinking"] = serde_json::json!({"type": thinking_type});
    }

    fn max_reasoning_level(&self) -> ReasoningLevel {
        ReasoningLevel::Max
    }
}

impl ChatApiDriver for LiteLlmDriver {
    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        _reasoning_enabled: bool,
    ) {
        // LiteLLM proxy does not support the `reasoning` parameter.
        // Extract effort and pass it as top-level `reasoning_effort` instead,
        // which LiteLLM forwards to the upstream OpenAI-compatible endpoint.
        if let Some(effort) = reasoning_config.and_then(|c| c.as_effort()) {
            request_body["reasoning_effort"] = serde_json::Value::String(effort);
        }
    }

    fn max_reasoning_level(&self) -> ReasoningLevel {
        ReasoningLevel::Max
    }
}

impl ChatApiDriver for VolcengineDriver {
    fn max_tokens_field(&self) -> &'static str {
        "max_output_tokens"
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    ) {
        // Volcengine 使用与 Zhipu 相同的 thinking.type 字段控制推理开关。
        // 优先使用 reasoning_config 中的显式配置，其次使用 reasoning_enabled 标志。
        let enabled = match reasoning_config {
            Some(ReasoningConfig::Bool(value)) => *value,
            Some(ReasoningConfig::ThinkingBudget(_)) => true,
            Some(ReasoningConfig::Object(_)) => {
                // Object 类型（如 {"effort": "medium"}）直接透传。
                if let Some(ReasoningConfig::Object(reasoning)) = reasoning_config {
                    request_body["reasoning"] = reasoning.clone();
                }
                return;
            }
            None => reasoning_enabled,
        };
        let thinking_type = if enabled { "enabled" } else { "disabled" };
        request_body["thinking"] = serde_json::json!({"type": thinking_type});
    }

    fn max_reasoning_level(&self) -> ReasoningLevel {
        ReasoningLevel::Medium
    }
}

impl ChatApiDriver for MinimaxDriver {
    fn max_tokens_field(&self) -> &'static str {
        "max_completion_tokens"
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    ) {
        let thinking_type = match reasoning_config {
            Some(ReasoningConfig::Object(value)) => value
                .get("type")
                .and_then(|v| v.as_str())
                .filter(|kind| matches!(*kind, "disabled" | "adaptive"))
                .unwrap_or(if reasoning_enabled {
                    "adaptive"
                } else {
                    "disabled"
                }),
            Some(ReasoningConfig::Bool(value)) => {
                if *value {
                    "adaptive"
                } else {
                    "disabled"
                }
            }
            Some(ReasoningConfig::ThinkingBudget(_)) => "adaptive",
            None => {
                if reasoning_enabled {
                    "adaptive"
                } else {
                    "disabled"
                }
            }
        };
        request_body["thinking"] = serde_json::json!({ "type": thinking_type });
        request_body["reasoning_split"] = serde_json::Value::Bool(true);
    }

    fn max_reasoning_level(&self) -> ReasoningLevel {
        ReasoningLevel::Medium
    }
}

impl ChatApiDriver for MimoDriver {
    fn max_tokens_field(&self) -> &'static str {
        "max_completion_tokens"
    }

    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        _reasoning_enabled: bool,
    ) {
        let enabled = match reasoning_config {
            Some(ReasoningConfig::Bool(value)) => *value,
            Some(ReasoningConfig::ThinkingBudget(_)) | Some(ReasoningConfig::Object(_)) => true,
            None => true,
        };
        let thinking_type = if enabled { "enabled" } else { "disabled" };
        request_body["thinking"] = serde_json::json!({ "type": thinking_type });
    }

    fn max_reasoning_level(&self) -> ReasoningLevel {
        ReasoningLevel::Medium
    }
}

pub(crate) fn driver_for_provider_driver(
    driver: ProviderDriverKind,
) -> Box<dyn ChatApiDriver + Send + Sync> {
    match driver {
        ProviderDriverKind::OpenAI => Box::new(OpenAiDriver),
        ProviderDriverKind::Zhipu => Box::new(ZhipuDriver),
        ProviderDriverKind::LiteLLM => Box::new(LiteLlmDriver),
        ProviderDriverKind::Volcengine => Box::new(VolcengineDriver),
        ProviderDriverKind::Minimax => Box::new(MinimaxDriver),
        ProviderDriverKind::Mimo => Box::new(MimoDriver),
        // Ollama 有专用 OllamaProvider，不经此 OpenAI 兼容驱动；兜底走 OpenAI 驱动。
        ProviderDriverKind::Anthropic | ProviderDriverKind::Ollama => Box::new(OpenAiDriver),
    }
}

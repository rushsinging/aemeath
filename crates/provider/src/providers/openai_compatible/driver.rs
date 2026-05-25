//! Chat API driver 抽象：不同供应商的推理字段差异化处理

use aemeath_core::provider::ApiDriverKind;

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
}

#[derive(Debug)]
pub struct OpenAiDriver;

#[derive(Debug)]
pub struct ZhipuDriver;

#[derive(Debug)]
pub struct LiteLlmDriver;

#[derive(Debug)]
pub struct VolcengineDriver;

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
}

impl ChatApiDriver for VolcengineDriver {
    fn max_tokens_field(&self) -> &'static str {
        "max_output_tokens"
    }

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

pub(crate) fn driver_for_api(api: ApiDriverKind) -> Box<dyn ChatApiDriver + Send + Sync> {
    match api {
        ApiDriverKind::OpenAI => Box::new(OpenAiDriver),
        ApiDriverKind::Zhipu => Box::new(ZhipuDriver),
        ApiDriverKind::LiteLLM => Box::new(LiteLlmDriver),
        ApiDriverKind::Volcengine => Box::new(VolcengineDriver),
        ApiDriverKind::Anthropic => Box::new(OpenAiDriver),
    }
}

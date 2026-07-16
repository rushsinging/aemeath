//! Chat Provider driver 抽象：不同供应商的推理字段差异化处理

use crate::ports::ReasoningLevel;
use crate::ProviderDriverKind;

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

    /// 将 effort 字符串映射到本 driver 支持的档位。
    ///
    /// 默认实现原样返回；各 driver 按自身能力覆盖，
    /// 将不支持的档位降级到最接近的可用值。
    fn clamp_effort<'a>(&self, effort: &'a str) -> &'a str {
        effort
    }

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

#[derive(Debug)]
pub struct DeepSeekDriver;

#[derive(Debug)]
pub struct AgnesDriver;

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

    /// OpenAI API 最高支持 high，xhigh/max 降级到 high。
    fn clamp_effort<'a>(&self, effort: &'a str) -> &'a str {
        match effort {
            "xhigh" | "max" => "high",
            _ => effort,
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

        // thinking 开启时附带 reasoning_effort 字段（仅 GLM-5.2 生效）。
        // 服务端兼容映射：low/medium → high, xhigh → max。
        if enabled {
            if let Some(effort) = reasoning_config.and_then(|c| c.as_effort()) {
                request_body["reasoning_effort"] = serde_json::Value::String(effort);
            }
        }
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

    /// Volcengine 最高支持 medium，high 以上降级到 medium。
    fn clamp_effort<'a>(&self, effort: &'a str) -> &'a str {
        match effort {
            "high" | "xhigh" | "max" => "medium",
            _ => effort,
        }
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

impl ChatApiDriver for DeepSeekDriver {
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

        // thinking 开启时附带 reasoning_effort 顶层字段。
        // DeepSeek 仅认 high/max，服务端做兼容映射：
        // low/medium → high, xhigh → max。
        if enabled {
            if let Some(effort) = reasoning_config.and_then(|c| c.as_effort()) {
                request_body["reasoning_effort"] = serde_json::Value::String(effort);
            }
        }
    }

    fn max_reasoning_level(&self) -> ReasoningLevel {
        ReasoningLevel::Max
    }
}

impl ChatApiDriver for AgnesDriver {
    fn apply_reasoning_fields(
        &self,
        request_body: &mut serde_json::Value,
        reasoning_config: Option<&ReasoningConfig>,
        reasoning_enabled: bool,
    ) {
        // Agnes 使用 vLLM chat_template_kwargs.enable_thinking 控制思考开关。
        // 仅支持开/关，不支持 effort 分级。
        let enabled = match reasoning_config {
            Some(ReasoningConfig::Bool(value)) => *value,
            Some(ReasoningConfig::ThinkingBudget(_)) | Some(ReasoningConfig::Object(_)) => true,
            None => reasoning_enabled,
        };
        request_body["chat_template_kwargs"] = serde_json::json!({ "enable_thinking": enabled });
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
        ProviderDriverKind::DeepSeek => Box::new(DeepSeekDriver),
        ProviderDriverKind::Agnes => Box::new(AgnesDriver),
        // Ollama 有专用 OllamaProvider，不经此 OpenAI 兼容驱动；兜底走 OpenAI 驱动。
        ProviderDriverKind::Anthropic | ProviderDriverKind::Ollama => Box::new(OpenAiDriver),
    }
}
